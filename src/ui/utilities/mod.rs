mod fps;
mod notifications;
mod weather;

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

pub(super) use weather::coordinates;

#[derive(Default)]
pub(super) struct Utilities {
    month: Option<Date>,
    ticker: Option<Task<()>>,
    fps: Option<fps::Feed>,
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
    fn fps_sample(&mut self) -> anyhow::Result<Option<f32>> {
        let feed = match &mut self.fps {
            Some(feed) => feed,
            None => self.fps.insert(fps::Feed::open()?),
        };
        Ok(feed.sample()?)
    }

    fn poll_fps(&mut self) {
        match self.fps_sample() {
            Ok(value) => {
                self.fps_error = None;
                append(&mut self.fps_history, value);
            }
            Err(e) => {
                self.fps = None;
                self.fps_history.clear();
                self.fps_error = Some(format!("Gamescope feed unavailable · {e}"));
            }
        }
    }
}

impl Dashboard {
    pub(super) fn sync_utilities(&mut self, cx: &mut Context<Self>) {
        let ticking = [Kind::WorldClock, Kind::Calendar, Kind::Fps]
            .into_iter()
            .any(|k| self.home.contains(k));
        if !self.home.contains(Kind::Fps) {
            self.utilities.fps = None;
            self.utilities.fps_history.clear();
            self.utilities.fps_error = None;
        }
        if !ticking {
            self.utilities.ticker = None;
        } else if self.utilities.ticker.is_none() {
            self.utilities.ticker = Some(cx.spawn(async move |this, cx| {
                loop {
                    gpui::Timer::after(Duration::from_secs(1)).await;
                    if this
                        .update(cx, |this, cx| {
                            if this.home.contains(Kind::Fps) {
                                this.utilities.poll_fps();
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        self.sync_weather(cx);
        self.sync_notifications(cx);
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
                let complete = result
                    .as_ref()
                    .is_ok_and(|r| r.weather.is_some() && r.aqi.is_some());
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
                                        "Refresh failed · showing last reading".into()
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
                gpui::Timer::after(Duration::from_secs(if complete { 900 } else { 300 })).await;
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
                    gpui::Timer::after(Duration::from_secs(30)).await;
                }
            }));
        }
    }

    pub(super) fn world_clock(&self, height: f32) -> impl IntoElement {
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
                    .child(col().flex_1().child(label(name).truncate()).when(
                        height >= 125.,
                        |it| {
                            it.children(
                                time.as_ref()
                                    .map(|z| quiet(z.strftime("%a, %b %d · %Z").to_string())),
                            )
                        },
                    ))
                    .child(
                        mono(time.map_or_else(|| "—".into(), |z| z.strftime("%H:%M").to_string()))
                            .text_size(r(if height >= 120. { 21. } else { 16. }))
                            .text_color(c(TEAL)),
                    ),
            );
        }
        clocks
    }

    pub(super) fn calendar(&self, height: f32, cx: &Context<Self>) -> impl IntoElement {
        let today = Zoned::now().date();
        let first = self.utilities.month.unwrap_or(today.first_of_month());
        let mut nav = row().justify_between().gap(r(4.)).flex_none();
        for (id, text, delta) in [
            ("prev-month", "‹".to_owned(), -1),
            ("this-month", first.strftime("%B %Y").to_string(), 0),
            ("next-month", "›".to_owned(), 1),
        ] {
            let view = cx.entity();
            nav = nav.child(bind(
                button(id, None).h(r(23.)).px(r(7.)).child(text),
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
                .when(active, |it| it.bg(c(VIOLET)).text_color(c(BACK)))
                .child(text)
        };
        grid =
            grid.child(row().text_color(c(MUTED)).children(
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

    pub(super) fn weather_widget(&self, height: f32) -> impl IntoElement {
        let state = &self.utilities;
        let mut tile = col()
            .id("weather-readings")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(5.));
        if let Some(report) = &state.report {
            tile = tile.child(label(report.place.name.clone()).truncate());
            if let Some(reading) = &report.weather {
                let (value, unit) = if self.preferences.fahrenheit {
                    (reading.temperature_2m * 1.8 + 32., "°F")
                } else {
                    (reading.temperature_2m, "°C")
                };
                tile = tile.child(
                    row()
                        .items_baseline()
                        .gap(r(8.))
                        .child(
                            mono(format!("{value:.0}{unit}"))
                                .text_size(r(if height > 100. { 28. } else { 20. }))
                                .text_color(c(AMBER)),
                        )
                        .child(label(weather::condition(reading.weather_code))),
                );
            } else {
                tile = tile.child(quiet("Weather unavailable"));
            }
            tile = tile.child(if let Some(aqi) = report.aqi {
                label(format!("US AQI {aqi:.0} · {}", weather::air_category(aqi)))
            } else {
                label("Air quality unavailable")
            });
            if let Some(at) = state.weather_at {
                tile = tile.child(quiet(format!(
                    "Open-Meteo / CAMS · {} min ago",
                    at.elapsed().as_secs() / 60
                )));
            }
        } else if state.weather_error.is_none() {
            tile = tile.child(quiet(if state.weather_query.is_empty() {
                "Choose a location in Settings → Widgets."
            } else {
                "Fetching weather and air quality…"
            }));
        }
        tile.children(state.weather_error.as_ref().map(|e| quiet(e.clone())))
    }

    pub(super) fn notification_widget(&self) -> impl IntoElement {
        let state = &self.utilities;
        let mut list = col()
            .id("notification-list")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(7.));
        if let Some(error) = &state.notification_error {
            list = list.child(quiet(error.clone()));
        } else if state.notices.is_empty() {
            list = list.child(quiet("Waiting for desktop notifications…"));
        }
        list.children(state.notices.iter().map(|n| {
            col()
                .flex_none()
                .gap(r(2.))
                .child(
                    row()
                        .justify_between()
                        .gap(r(5.))
                        .child(quiet(n.app.clone()).truncate())
                        .child(quiet(n.time.clone())),
                )
                .child(label(n.title.clone()))
                .when(!n.body.is_empty(), |it| it.child(quiet(n.body.clone())))
        }))
    }

    pub(super) fn clear_notifications(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        bind(
            button("clear-notifications", None)
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

    pub(super) fn fps_widget(&self, height: f32) -> impl IntoElement {
        let state = &self.utilities;
        let mut tile = col().flex_1().gap(r(5.));
        if let Some(value) = state.fps_history.back() {
            tile = tile.child(
                mono(format!("{value:.1} FPS"))
                    .text_size(r(24.))
                    .text_color(c(TEAL)),
            );
            if height >= 65. {
                tile = tile.child(graph(
                    state.fps_history.clone(),
                    None,
                    state.fps_history.iter().copied().fold(60., f32::max),
                    TEAL,
                ));
            }
            tile.child(quiet(format!("Gamescope · last {HISTORY} seconds")))
        } else {
            let status = state
                .fps_error
                .as_deref()
                .unwrap_or("Waiting for Gamescope. Choose Setup for the Steam launch options.");
            tile.child(quiet(status.to_owned()))
        }
    }
}
