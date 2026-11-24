mod calendar;
mod fps;
mod integration_status;
mod notifications;
mod weather;
mod weather_chart;
mod weather_tile;

use super::{Dashboard, HISTORY, Section, append, home::Kind, monitor::graph, theme::*};
use futures_lite::StreamExt;
use gpui::{Context, IntoElement, Styled, Task, prelude::*};
use jiff::{Timestamp, Zoned, tz::TimeZone};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, SystemTime},
};

pub(super) fn clock_zones(saved: &Option<Vec<String>>) -> Vec<String> {
    saved
        .clone()
        .unwrap_or_else(|| vec!["UTC".into(), "Europe/London".into(), "Asia/Tokyo".into()])
}

pub(super) fn parse_zones(value: &str) -> anyhow::Result<Vec<String>> {
    let mut zones = Vec::new();
    for name in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        anyhow::ensure!(
            TimeZone::get(name).is_ok(),
            "Unknown time zone “{name}”. Use an IANA name such as Europe/London."
        );
        if !zones.iter().any(|zone| zone == name) {
            zones.push(name.to_owned());
        }
    }
    Ok(zones)
}

pub(super) use fps::LAUNCH_OPTIONS as FPS_LAUNCH_OPTIONS;
pub(super) use weather::coordinates;

#[derive(Default)]
pub(super) struct Utilities {
    calendar: calendar::Calendar,
    ticker: Option<(bool, Task<()>)>,
    fps_task: Option<Task<()>>,
    fps_setup_task: Option<Task<()>>,
    fps_setup: Option<fps::Setup>,
    fps_reading: Option<fps::Reading>,
    fps_history: VecDeque<f32>,
    fps_error: Option<String>,
    weather_query: String,
    weather_task: Option<Task<()>>,
    report: Option<weather::CachedReport>,
    weather_error: Option<String>,
    notices: VecDeque<notifications::Notice>,
    pub(super) notification_apps: BTreeMap<String, String>,
    notification_task: Option<Task<()>>,
    notification_status: notifications::Status,
}

impl Utilities {
    pub(super) fn new() -> Self {
        if let Err(error) = fps::prepare_mangohud() {
            log::warn!("Could not prepare MangoHud logging: {error:#}");
        }
        Self::default()
    }

    fn record_fps(&mut self, result: anyhow::Result<Option<fps::Reading>>) -> bool {
        let error = result
            .as_ref()
            .err()
            .map(|e| format!("FPS feed unavailable · {e:#}"));
        let reading = result.ok().flatten();
        let redraw = self.fps_reading.is_some() || reading.is_some() || self.fps_error != error;
        if reading.as_ref().map(|r| r.source) != self.fps_reading.as_ref().map(|r| r.source) {
            self.fps_history.clear();
        }
        append(&mut self.fps_history, reading.as_ref().map(|r| r.current));
        self.fps_reading = reading;
        self.fps_error = error;
        redraw
    }

    fn record_weather(
        &mut self,
        mut report: weather::Report,
        now: SystemTime,
    ) -> &weather::CachedReport {
        let complete = report.is_complete();
        let mut updated_at = now;
        self.weather_error = None;
        if report.weather.is_none()
            && let Some(previous) = self.report.take()
            && previous.report.weather.is_some()
        {
            report.weather = previous.report.weather;
            updated_at = previous.updated_at;
            self.weather_error = Some("Forecast refresh failed · showing the last forecast".into());
        }
        self.report.insert(weather::CachedReport {
            report,
            updated_at,
            fetched_at: now,
            complete,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_redraws_on_feed_changes_and_samples_only() {
        let mut utilities = Utilities::default();
        assert!(!utilities.record_fps(Ok(None)));
        assert!(utilities.record_fps(Err(anyhow::anyhow!("offline"))));
        assert!(!utilities.record_fps(Err(anyhow::anyhow!("offline"))));
        for _ in 0..2 {
            assert!(utilities.record_fps(Ok(Some(fps::Reading {
                source: fps::Source::Gamescope,
                current: 60.,
                low_1: None,
                low_01: None,
            }))));
        }
        assert_eq!(utilities.fps_history.len(), 2);
        assert!(utilities.record_fps(Ok(None)));
        assert!(utilities.fps_history.is_empty());
        assert!(!utilities.record_fps(Ok(None)));
    }
}

impl Dashboard {
    pub(super) fn sync_utilities(&mut self, cx: &mut Context<Self>) {
        let clock = self.home.shows(Kind::WorldClock);
        if !clock && !self.home.shows(Kind::Calendar) {
            self.utilities.ticker = None;
        } else if self
            .utilities
            .ticker
            .as_ref()
            .is_none_or(|(previous, _)| *previous != clock)
        {
            let ticker = cx.spawn(async move |this, cx| {
                let mut date = Zoned::now().date();
                loop {
                    let now = Timestamp::now();
                    const MINUTE_NS: i128 = 60_000_000_000;
                    let elapsed_ns = now.as_nanosecond().rem_euclid(MINUTE_NS);
                    let until_minute = Duration::from_nanos((MINUTE_NS - elapsed_ns) as u64);
                    cx.background_executor().timer(until_minute).await;
                    let next = Zoned::now().date();
                    if (clock || next != date) && this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                    date = next;
                }
            });
            self.utilities.ticker = Some((clock, ticker));
        }
        self.sync_calendar(cx);
        self.sync_fps(cx);
        self.sync_weather(cx);
        self.sync_notifications(cx);
    }

    fn sync_fps(&mut self, cx: &mut Context<Self>) {
        let checking = self.showing_settings(Section::Integrations);
        if !checking {
            self.utilities.fps_setup_task = None;
            self.utilities.fps_setup = None;
        } else if self.utilities.fps_setup_task.is_none() {
            self.utilities.fps_setup_task = Some(cx.spawn(async move |this, cx| {
                let setup = blocking::unblock(fps::Setup::check).await;
                let _ = this.update(cx, |this, cx| {
                    this.utilities.fps_setup = Some(setup);
                    cx.notify();
                });
            }));
        }
        if !self.home.contains(Kind::Fps) && !checking {
            if self.utilities.fps_task.take().is_some() {
                self.utilities.record_fps(Ok(None));
            }
        } else if self.utilities.fps_task.is_none() {
            self.utilities.fps_task = Some(cx.spawn(async move |this, cx| {
                let mut feed: Option<fps::Feed> = None;
                loop {
                    let (next_feed, reading) = blocking::unblock(move || {
                        let reading = match &mut feed {
                            Some(feed) => feed.sample(),
                            None => {
                                fps::Feed::open().and_then(|opened| feed.insert(opened).sample())
                            }
                        };
                        (feed, reading)
                    })
                    .await;
                    feed = next_feed;
                    if this
                        .update(cx, |this, cx| {
                            let previous = this
                                .showing_settings(Section::Integrations)
                                .then(|| this.fps_integration_status());
                            let redraw = this.utilities.record_fps(reading);
                            if (redraw && this.home.shows(Kind::Fps))
                                || previous
                                    .is_some_and(|status| status != this.fps_integration_status())
                            {
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                }
            }));
        }
    }

    fn sync_weather(&mut self, cx: &mut Context<Self>) {
        let query = self.preferences.weather_location.trim();
        if self.utilities.weather_query != query {
            self.utilities.weather_query = query.into();
            self.utilities.weather_task = None;
            self.utilities.report = None;
            self.utilities.weather_error = None;
        }
        if query.is_empty()
            || !self.home.contains(Kind::Weather)
            || self.utilities.weather_task.is_some()
        {
            return;
        }
        let query = query.to_owned();
        let cached_delay = self
            .utilities
            .report
            .as_ref()
            .map(|cached| cached.refresh_delay(SystemTime::now()));
        self.utilities.weather_task = Some(cx.spawn(async move |this, cx| {
            let cache_path = weather::cache_path(&query);
            let mut delay = cached_delay.unwrap_or_default();
            if let (None, Some(path)) = (cached_delay, cache_path.clone())
                && let Some(cached) =
                    blocking::unblock(move || weather::CachedReport::load(&path)).await
            {
                delay = cached.refresh_delay(SystemTime::now());
                if this
                    .update(cx, |this, cx| {
                        this.utilities.report = Some(cached);
                        if this.home.shows(Kind::Weather) {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
            loop {
                if !delay.is_zero() {
                    cx.background_executor().timer(delay).await;
                }
                let Ok(Some(known_place)) = this.update(cx, |this, _| {
                    if !this.home.contains(Kind::Weather) {
                        // Let in-flight refreshes finish, but start no new requests while hidden.
                        this.utilities.weather_task = None;
                        return None;
                    }
                    Some(
                        this.utilities
                            .report
                            .as_ref()
                            .map(|cached| cached.report.place.clone()),
                    )
                }) else {
                    break;
                };
                let query = query.clone();
                let result = blocking::unblock(move || weather::fetch(&query, known_place)).await;
                let Ok(cached) = this.update(cx, |this, cx| {
                    let cached = match result {
                        Ok(report) => Some(
                            this.utilities
                                .record_weather(report, SystemTime::now())
                                .clone(),
                        ),
                        Err(e) => {
                            this.utilities.weather_error =
                                Some(if this.utilities.report.is_some() {
                                    "Refresh failed · showing the last forecast".into()
                                } else {
                                    e.to_string()
                                });
                            None
                        }
                    };
                    if this.home.shows(Kind::Weather) {
                        cx.notify();
                    }
                    cached
                }) else {
                    break;
                };
                delay = weather::RETRY_INTERVAL;
                if let Some(cached) = cached {
                    delay = cached.refresh_delay(SystemTime::now());
                    if let Some(path) = cache_path.clone() {
                        blocking::unblock(move || cached.save(&path)).await;
                    }
                }
            }
        }));
    }

    fn sync_notifications(&mut self, cx: &mut Context<Self>) {
        let has_tile = self.home.contains(Kind::Notifications);
        if !has_tile {
            self.utilities.notices.clear();
        }
        let active = has_tile
            || self.showing_settings(Section::Notifications)
            || self.showing_settings(Section::Integrations);
        if !active {
            self.utilities.notification_task = None;
            self.utilities.notification_status = notifications::Status::Connecting;
        } else if self.utilities.notification_task.is_none() {
            self.utilities.notification_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    let status = match notifications::listen().await {
                        Ok(mut stream) => {
                            if this
                                .update(cx, |this, cx| {
                                    this.set_notification_status(
                                        notifications::Status::Listening,
                                        cx,
                                    );
                                })
                                .is_err()
                            {
                                break;
                            }
                            while let Some(Ok(msg)) = stream.next().await {
                                if this
                                    .update(cx, |this, cx| this.receive_notification(msg, cx))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            notifications::Status::Disconnected
                        }
                        Err(error) => {
                            log::debug!("Desktop notification feed unavailable: {error}");
                            notifications::Status::from_error(&error)
                        }
                    };
                    if this
                        .update(cx, |this, cx| {
                            this.set_notification_status(status, cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_secs(30))
                        .await;
                }
            }));
        }
    }

    fn set_notification_status(&mut self, status: notifications::Status, cx: &mut Context<Self>) {
        if self.utilities.notification_status == status {
            return;
        }
        self.utilities.notification_status = status;
        if self.home.shows(Kind::Notifications)
            || self.showing_settings(Section::Notifications)
            || self.showing_settings(Section::Integrations)
        {
            cx.notify();
        }
    }

    fn receive_notification(&mut self, msg: zbus::Message, cx: &mut Context<Self>) {
        let has_tile = self.home.contains(Kind::Notifications);
        let editing_filters = self.showing_settings(Section::Notifications);
        if !(has_tile || editing_filters) {
            return;
        }
        let Some(notice) = notifications::notice(&msg) else {
            return;
        };
        let apps = &mut self.utilities.notification_apps;
        let app_changed = apps.get(&notice.key) != Some(&notice.app);
        if app_changed {
            apps.insert(notice.key.clone(), notice.app.clone());
        }
        let show_notice = has_tile
            && !self
                .preferences
                .hidden_notification_apps
                .contains_key(&notice.key);
        if show_notice {
            let notices = &mut self.utilities.notices;
            notices.truncate(29);
            notices.push_front(notice);
        }
        if (show_notice && self.home.shows(Kind::Notifications)) || (app_changed && editing_filters)
        {
            cx.notify();
        }
    }

    pub(super) fn apply_notification_filters(&mut self) {
        let hidden = &self.preferences.hidden_notification_apps;
        self.utilities
            .notices
            .retain(|notice| !hidden.contains_key(&notice.key));
    }

    pub(super) fn notification_status_message(&self) -> &'static str {
        self.utilities.notification_status.message()
    }

    pub(super) fn world_clock(&self, theme: Theme, height: f32) -> impl IntoElement {
        let now = Zoned::now();
        let mut clocks = col()
            .id("world-times")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(5.));
        for (name, time) in std::iter::once(("Local time".to_owned(), Some(now.clone()))).chain(
            clock_zones(&self.preferences.clock_zones)
                .into_iter()
                .map(|name| {
                    let time = now.in_tz(&name).ok();
                    (name.replace('_', " "), time)
                }),
        ) {
            clocks = clocks.child(
                row()
                    .flex_none()
                    .gap(r(7.))
                    .justify_between()
                    .child(col().flex_1().child(theme.label(name).truncate()).when(
                        height >= 125.,
                        |it| {
                            it.children(
                                time.as_ref()
                                    .map(|z| theme.quiet(z.strftime("%a, %b %d · %Z").to_string())),
                            )
                        },
                    ))
                    .child(
                        mono(time.map_or_else(|| "—".into(), |z| z.strftime("%H:%M").to_string()))
                            .text_size(r(if height >= 120. { 21. } else { 16. }))
                            .text_color(theme.color(TEAL)),
                    ),
            );
        }
        clocks
    }

    pub(super) fn notification_widget(&self, theme: Theme) -> impl IntoElement {
        let state = &self.utilities;
        let mut list = col()
            .id("notification-list")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(7.));
        if state.notification_status != notifications::Status::Listening {
            list = list.child(theme.quiet(state.notification_status.message()));
        } else if state.notices.is_empty() {
            list = list.child(theme.quiet("Waiting for desktop notifications…"));
        }
        list.children(state.notices.iter().map(|n| {
            col()
                .flex_none()
                .gap(r(2.))
                .child(
                    row()
                        .justify_between()
                        .gap(r(5.))
                        .child(theme.quiet(n.app.clone()).truncate())
                        .child(theme.quiet(n.time.clone())),
                )
                .child(theme.label(n.title.clone()))
                .when(
                    !self.preferences.hide_notification_bodies && !n.body.is_empty(),
                    |it| it.child(theme.quiet(n.body.clone())),
                )
        }))
    }

    pub(super) fn clear_notifications(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity();
        theme.bind(
            theme
                .button("clear-notifications", None)
                .h(r(26.))
                .px(r(7.))
                .child("Clear"),
            move |_, cx| {
                view.update(cx, |d, cx| {
                    d.utilities.notices.clear();
                    cx.notify();
                })
            },
        )
    }

    pub(super) fn fps_widget(
        &self,
        id: u32,
        theme: Theme,
        width: f32,
        height: f32,
    ) -> impl IntoElement {
        let state = &self.utilities;
        let tile = col().flex_1().gap(r(6.));
        if let Some(reading) = &state.fps_reading {
            let compact = height < 85.;
            let gap = 8.;
            let metric_width = (width - 2. * gap) / 3.;
            let metric = |name, value: Option<f32>, color, size: f32| {
                let text = value.map_or_else(|| "—".into(), |fps| format!("{fps:.1}"));
                // Leave enough room for each monospace digit at narrow tile sizes.
                let fitted_size = size.min(metric_width / (0.64 * text.chars().count() as f32));
                col()
                    .flex_1()
                    .gap(r(2.))
                    .child(theme.quiet(name).whitespace_nowrap())
                    .child(
                        mono(text)
                            .text_size(r(fitted_size.max(10.)))
                            .text_color(theme.color(color))
                            .truncate(),
                    )
            };
            tile.child(
                split()
                    .gap(r(gap))
                    .flex_none()
                    .child(metric(
                        "Current",
                        Some(reading.current),
                        TEAL,
                        if compact { 17. } else { 24. },
                    ))
                    .child(metric(
                        "1% low",
                        reading.low_1,
                        INK,
                        if compact { 14. } else { 19. },
                    ))
                    .child(metric(
                        "0.1% low",
                        reading.low_01,
                        INK,
                        if compact { 14. } else { 19. },
                    )),
            )
            .when(height >= 75., |it| {
                it.child(graph(
                    (id, 0),
                    theme,
                    state.fps_history.clone(),
                    None,
                    state.fps_history.iter().copied().fold(60., f32::max),
                    TEAL,
                ))
            })
            .when(height >= 100., |it| {
                it.child(
                    theme
                        .quiet(match reading.source {
                            fps::Source::MangoHud(_) => {
                                format!("MangoHud · last {HISTORY} seconds")
                            }
                            fps::Source::Gamescope => "Gamescope · Use MangoHud for lows".into(),
                        })
                        .truncate()
                        .flex_none(),
                )
            })
        } else {
            let status = state
                .fps_error
                .as_deref()
                .unwrap_or("Waiting for a game. See Settings → Widgets for MangoHud setup.");
            tile.child(theme.quiet(status.to_owned()))
        }
    }
}
