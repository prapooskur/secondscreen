mod fps;
mod notifications;
mod weather;
mod weather_chart;
mod weather_tile;

use super::{Dashboard, HISTORY, append, home::Kind, monitor::graph, theme::*};
use futures_lite::StreamExt;
use gpui::{Context, IntoElement, Styled, Task, div, prelude::*};
use jiff::{Span, Zoned, civil::Date, tz::TimeZone};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
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
    month: Option<Date>,
    ticker: Option<Task<()>>,
    fps_task: Option<Task<()>>,
    fps_reading: Option<fps::Reading>,
    fps_history: VecDeque<f32>,
    fps_error: Option<String>,
    weather_query: String,
    weather_task: Option<Task<()>>,
    report: Option<weather::Report>,
    weather_error: Option<String>,
    weather_at: Option<Instant>,
    notices: VecDeque<notifications::Notice>,
    notification_task: Option<Task<()>>,
    notification_error: Option<String>,
}

impl Utilities {
    pub(super) fn new() -> Self {
        if let Err(error) = fps::prepare_mangohud() {
            log::warn!("Could not prepare MangoHud logging: {error:#}");
        }
        Self::default()
    }

    fn record_fps(&mut self, result: anyhow::Result<Option<fps::Reading>>) {
        self.fps_error = result
            .as_ref()
            .err()
            .map(|e| format!("FPS feed unavailable · {e:#}"));
        let reading = result.ok().flatten();
        if reading.as_ref().map(|r| r.source) != self.fps_reading.as_ref().map(|r| r.source) {
            self.fps_history.clear();
        }
        append(&mut self.fps_history, reading.as_ref().map(|r| r.current));
        self.fps_reading = reading;
    }
}

impl Dashboard {
    pub(super) fn sync_utilities(&mut self, cx: &mut Context<Self>) {
        let ticking = [Kind::WorldClock, Kind::Calendar]
            .into_iter()
            .any(|k| self.home.contains(k));
        if !ticking {
            self.utilities.ticker = None;
        } else if self.utilities.ticker.is_none() {
            self.utilities.ticker = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
        self.sync_fps(cx);
        self.sync_weather(cx);
        self.sync_notifications(cx);
    }

    fn sync_fps(&mut self, cx: &mut Context<Self>) {
        if !self.home.contains(Kind::Fps) {
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
                            this.utilities.record_fps(reading);
                            cx.notify();
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
        let query = if self.home.contains(Kind::Weather) {
            self.preferences.weather_location.trim()
        } else {
            ""
        };
        if self.utilities.weather_query == query {
            return;
        }
        self.utilities.weather_query = query.into();
        self.utilities.weather_task = None;
        self.utilities.report = None;
        self.utilities.weather_error = None;
        self.utilities.weather_at = None;
        if query.is_empty() {
            return;
        }
        let query = query.to_owned();
        self.utilities.weather_task = Some(cx.spawn(async move |this, cx| {
            let mut place = None;
            loop {
                let query = query.clone();
                let known = place.clone();
                let result = blocking::unblock(move || weather::fetch(&query, known)).await;
                let complete = result.as_ref().is_ok_and(|r| {
                    r.weather.as_ref().is_some_and(|w| {
                        w.current.is_some() && !w.days.is_empty() && w.has_hourly_temperatures()
                    }) && r.aqi.is_some()
                });
                if this
                    .update(cx, |this, cx| {
                        match result {
                            Ok(report) => {
                                place = Some(report.place.clone());
                                this.utilities.report = Some(report);
                                this.utilities.weather_at = Some(Instant::now());
                                this.utilities.weather_error = None;
                            }
                            Err(e) => {
                                this.utilities.weather_error =
                                    Some(if this.utilities.report.is_some() {
                                        "Refresh failed · showing the last forecast".into()
                                    } else {
                                        e.to_string()
                                    })
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                let delay = Duration::from_secs(if complete { 900 } else { 300 });
                cx.background_executor().timer(delay).await;
            }
        }));
    }

    fn sync_notifications(&mut self, cx: &mut Context<Self>) {
        if !self.home.contains(Kind::Notifications) {
            self.utilities.notification_task = None;
            self.utilities.notification_error = None;
            self.utilities.notices.clear();
        } else if self.utilities.notification_task.is_none() {
            self.utilities.notification_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    let error = match notifications::listen().await {
                        Ok(mut stream) => {
                            if this
                                .update(cx, |this, cx| {
                                    this.utilities.notification_error = None;
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                            while let Some(Ok(msg)) = stream.next().await {
                                if let Some(notice) = notifications::notice(&msg)
                                    && this
                                        .update(cx, |this, cx| {
                                            let notices = &mut this.utilities.notices;
                                            notices.truncate(29);
                                            notices.push_front(notice);
                                            cx.notify();
                                        })
                                        .is_err()
                                {
                                    return;
                                }
                            }
                            "Notification bus disconnected".into()
                        }
                        Err(e) => format!("Desktop notification feed unavailable · {e}"),
                    };
                    if this
                        .update(cx, |this, cx| {
                            this.utilities.notification_error = Some(error);
                            cx.notify();
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

    pub(super) fn calendar(&self, height: f32, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let today = Zoned::now().date();
        let first = self.utilities.month.unwrap_or(today.first_of_month());
        let mut nav = row().justify_between().gap(r(4.)).flex_none();
        for (id, text, delta) in [
            ("prev-month", "‹".to_owned(), -1),
            ("this-month", first.strftime("%B %Y").to_string(), 0),
            ("next-month", "›".to_owned(), 1),
        ] {
            let view = cx.entity();
            nav = nav.child(theme.bind(
                theme.button(id, None).h(r(23.)).px(r(7.)).child(text),
                move |_, cx| {
                    view.update(cx, |d, cx| {
                        d.utilities.month = if delta == 0 {
                            None
                        } else {
                            first.checked_add(Span::new().months(delta)).ok()
                        };
                        cx.notify();
                    })
                },
            ));
        }
        let mut grid = col().id("calendar-days").flex_1().overflow_y_scroll();
        let cell = |text: String, active: bool| {
            div()
                .flex_1()
                .text_center()
                .text_size(r(if height < 120. { 11. } else { 13. }))
                .rounded(r(4.))
                .when(active, |it| {
                    it.bg(theme.color(VIOLET)).text_color(theme.color(BACK))
                })
                .child(text)
        };
        grid =
            grid.child(row().text_color(theme.color(MUTED)).children(
                ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|v| cell(v.into(), false)),
            ));
        let start = first.weekday().to_monday_zero_offset();
        for week in 0..(start + first.days_in_month() + 6) / 7 {
            grid = grid.child(row().flex_none().children((0..7).map(|col| {
                let day = week * 7 + col - start + 1;
                let valid = (1..=first.days_in_month()).contains(&day);
                cell(
                    if valid {
                        day.to_string()
                    } else {
                        String::new()
                    },
                    first == today.first_of_month() && day == today.day(),
                )
            })));
        }
        col().flex_1().gap(r(3.)).child(nav).child(grid)
    }

    pub(super) fn notification_widget(&self, theme: Theme) -> impl IntoElement {
        let state = &self.utilities;
        let mut list = col()
            .id("notification-list")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(7.));
        if let Some(error) = &state.notification_error {
            list = list.child(theme.quiet(error.clone()));
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
                .when(!n.body.is_empty(), |it| {
                    it.child(theme.quiet(n.body.clone()))
                })
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

    pub(super) fn fps_widget(&self, theme: Theme, width: f32, height: f32) -> impl IntoElement {
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
