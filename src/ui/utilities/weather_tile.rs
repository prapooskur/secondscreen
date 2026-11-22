use gpui::{Div, IntoElement, Styled, div, linear_color_stop, linear_gradient, prelude::*};

use super::weather;
use crate::ui::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    theme::*,
};

impl Dashboard {
    pub(in crate::ui) fn weather_widget(&self, columns: u8, height: f32) -> impl IntoElement {
        let state = &self.utilities;
        let mut tile = col()
            .id("weather-readings")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(8.));
        let Some(report) = &state.report else {
            let message = state.weather_error.as_deref().unwrap_or_else(|| {
                if state.weather_query.is_empty() {
                    "Choose a location in Settings → Widgets."
                } else {
                    "Fetching the forecast and air quality…"
                }
            });
            return tile.child(quiet(message.to_owned()));
        };

        let forecast = report.weather.as_ref();
        let current = forecast.and_then(|weather| weather.current.as_ref());
        let today = forecast.and_then(|weather| {
            weather
                .days
                .first()
                .filter(|day| Some(day.date) == weather.today)
        });
        let fahrenheit = self.preferences.fahrenheit;
        let wide = columns >= 2;
        let code = current.and_then(|reading| reading.weather_code);
        let daylight = current.and_then(|reading| reading.is_day) != Some(0);
        let (symbol, accent) = weather_symbol(code, daylight);
        let summary = col()
            .flex_1()
            .justify_center()
            .gap(r(2.))
            .child(label(report.place.name.clone()).truncate())
            .child(
                row()
                    .gap(r(9.))
                    .child(icon(
                        symbol,
                        c(accent),
                        if wide && height >= 160. { 40. } else { 29. },
                    ))
                    .child(
                        mono(temperature(
                            current.and_then(|reading| reading.temperature_2m),
                            fahrenheit,
                            true,
                        ))
                        .flex_none()
                        .text_size(r(if wide && height >= 160. { 35. } else { 25. }))
                        .text_color(c(INK)),
                    )
                    .child(quiet(weather::condition(code)).flex_1()),
            );

        let wind = current
            .and_then(|reading| reading.wind_speed_10m)
            .map(|speed| {
                if self.preferences.weather_mph {
                    format!("{:.0} mph", speed * 0.621_371)
                } else {
                    format!("{speed:.0} km/h")
                }
            });
        let time = |time: jiff::civil::Time| {
            time.strftime(if self.preferences.weather_12_hour {
                "%-I:%M %p"
            } else {
                "%H:%M"
            })
            .to_string()
        };
        let details = row()
            .flex_none()
            .gap(r(10.))
            .justify_between()
            .child(detail("Wind", wind))
            .child(detail(
                "Sunrise",
                today.and_then(|day| day.sunrise).map(time),
            ))
            .child(detail("Sunset", today.and_then(|day| day.sunset).map(time)));

        let (hero_details, footer_details) = if wide {
            (Some(details), None)
        } else {
            (None, Some(details))
        };
        let hero = row()
            .flex_none()
            .p(r(8.))
            .gap(r(12.))
            .rounded(r(8.))
            .bg(linear_gradient(
                110.,
                linear_color_stop(c(accent).opacity(0.13), 0.),
                linear_color_stop(c(HEADER), 1.),
            ))
            .child(summary)
            .children(hero_details);
        tile = tile.child(hero);

        let air = report.aqi.map_or_else(
            || "Air quality unavailable".into(),
            |aqi| format!("US AQI {aqi:.0} · {}", weather::air_category(aqi)),
        );
        tile = tile.child(quiet(air));
        if let Some(forecast) = forecast.filter(|weather| !weather.days.is_empty()) {
            let heading = label(format!("{}-day forecast", forecast.days.len()));
            if wide {
                tile = tile.child(heading);
                for days in forecast.days.chunks(if columns >= 4 { 10 } else { 5 }) {
                    tile = tile.child(split().flex_none().gap(r(6.)).children(days.iter().map(
                        |day| forecast_card(day, Some(day.date) == forecast.today, fahrenheit),
                    )));
                }
            } else {
                tile = tile.child(
                    row()
                        .flex_none()
                        .child(heading.flex_1())
                        .child(temperature_column("High", INK))
                        .child(temperature_column("Low", MUTED)),
                );
                for day in &forecast.days {
                    tile = tile.child(forecast_row(
                        day,
                        Some(day.date) == forecast.today,
                        fahrenheit,
                    ));
                }
            }
        } else {
            tile = tile.child(quiet("Daily forecast unavailable"));
        }
        tile.children(footer_details)
            .children(
                state
                    .weather_error
                    .as_ref()
                    .map(|error| quiet(error.clone())),
            )
            .children(state.weather_at.map(|at| {
                quiet(format!(
                    "Open-Meteo / CAMS · {} min ago",
                    at.elapsed().as_secs() / 60
                ))
            }))
    }
}

fn detail(name: &'static str, value: Option<String>) -> Div {
    col()
        .flex_none()
        .gap(r(2.))
        .child(quiet(name))
        .child(label(value.unwrap_or_else(|| "—".into())).whitespace_nowrap())
}

fn day_label(day: &weather::Day, today: bool) -> String {
    if today {
        "Today".into()
    } else {
        day.date.strftime("%a").to_string()
    }
}

fn forecast_card(day: &weather::Day, today: bool, fahrenheit: bool) -> impl IntoElement {
    let (symbol, accent) = weather_symbol(day.weather_code, true);
    let condition = weather::condition(day.weather_code);
    col()
        .id(("weather-day", day.date.day_of_year() as usize))
        .flex_1()
        .items_center()
        .gap(r(2.))
        .py(r(5.))
        .rounded(r(7.))
        .border_1()
        .border_color(c(STROKE))
        .bg(c(HEADER))
        .when(today, |card| card.border_color(c(AMBER).opacity(0.46)))
        .child(label(day_label(day, today)).text_color(c(INK)))
        .child(quiet(day.date.strftime("%b %e").to_string()))
        .child(icon(symbol, c(accent), 22.))
        .child(
            row()
                .gap(r(5.))
                .child(mono(temperature(day.high, fahrenheit, false)).text_color(c(INK)))
                .child(mono(temperature(day.low, fahrenheit, false)).text_color(c(MUTED))),
        )
        .tooltip(move |_, cx| cx.new(|_| Tooltip(condition.into())).into())
}

fn forecast_row(day: &weather::Day, today: bool, fahrenheit: bool) -> impl IntoElement {
    let (symbol, accent) = weather_symbol(day.weather_code, true);
    let condition = weather::condition(day.weather_code);
    row()
        .id(("weather-day", day.date.day_of_year() as usize))
        .flex_none()
        .h(r(25.))
        .rounded(r(5.))
        .when(today, |line| line.bg(c(AMBER).opacity(0.08)))
        .child(
            row()
                .flex_1()
                .gap(r(4.))
                .child(label(day_label(day, today)).w(r(41.)).truncate())
                .child(quiet(day.date.strftime("%b %e").to_string()).truncate()),
        )
        .child(icon(symbol, c(accent), 17.))
        .child(temperature_column(
            temperature(day.high, fahrenheit, false),
            INK,
        ))
        .child(temperature_column(
            temperature(day.low, fahrenheit, false),
            MUTED,
        ))
        .tooltip(move |_, cx| cx.new(|_| Tooltip(condition.into())).into())
}

fn temperature_column(value: impl IntoElement, color: u32) -> Div {
    div()
        .flex_none()
        .w(r(37.))
        .text_right()
        .text_color(c(color))
        .child(mono(value).text_size(r(12.)))
}

fn temperature(value: Option<f32>, fahrenheit: bool, show_unit: bool) -> String {
    let Some(celsius) = value else {
        return "—".into();
    };
    let (value, unit) = if fahrenheit {
        (celsius * 1.8 + 32., "F")
    } else {
        (celsius, "C")
    };
    format!("{:.0}°{}", value.round(), if show_unit { unit } else { "" })
}

fn weather_symbol(code: Option<u8>, daylight: bool) -> (Icon, u32) {
    match code {
        Some(0) if !daylight => (Icon::Moon, BLUE),
        Some(0) => (Icon::Sun, AMBER),
        Some(1 | 2) if daylight => (Icon::PartlyCloudy, AMBER),
        Some(1..=3) => (Icon::Cloud, TEXT),
        Some(45 | 48) => (Icon::Fog, TEXT),
        Some(51..=67 | 80..=82) => (Icon::Rain, BLUE),
        Some(71..=77 | 85 | 86) => (Icon::Snow, 0xbce9ff),
        Some(95..=99) => (Icon::Storm, VIOLET),
        _ => (Icon::Cloud, MUTED),
    }
}
