use gpui::{
    Div, FontWeight, IntoElement, Styled, div, hsla, linear_color_stop, linear_gradient,
    prelude::*, relative, rgb,
};

use super::{
    weather::{self, temperature},
    weather_chart::today_chart,
};
use crate::ui::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    theme::*,
};

impl Dashboard {
    pub(in crate::ui) fn weather_widget(
        &self,
        theme: Theme,
        width: f32,
        height: f32,
    ) -> impl IntoElement {
        let state = &self.utilities;
        let tile = col()
            .id("weather-readings")
            .flex_1()
            .overflow_y_scroll()
            .p(r(10.))
            .gap(r(10.));
        let Some(report) = &state.report else {
            let message = state.weather_error.as_deref().unwrap_or_else(|| {
                if state.weather_query.is_empty() {
                    "Choose a location in Settings → Widgets."
                } else {
                    "Fetching the forecast and air quality…"
                }
            });
            return tile.child(theme.quiet(message.to_owned()));
        };

        let forecast = report.weather.as_ref();
        let today = forecast.and_then(|weather| {
            weather
                .days
                .iter()
                .find(|day| Some(day.date) == weather.today)
        });
        let fahrenheit = self.preferences.fahrenheit;
        let clock_12h = self.preferences.weather_12_hour;
        let current = forecast.and_then(|weather| weather.current.as_ref());
        let wide = width >= 720.;
        let split_forecast = width >= 460.;
        let compact = height < 245.;
        let summary = summary(
            theme,
            &report.place.name,
            current,
            today,
            fahrenheit,
            compact,
        );
        let chart_height = if wide {
            (height - 99.).clamp(90., 242.)
        } else if compact {
            88.
        } else {
            112.
        };
        let chart = today_chart(theme, forecast, chart_height, fahrenheit, clock_12h);
        let days = daily_forecast(theme, forecast, fahrenheit, compact);
        let details = weather_details(
            theme,
            report.aqi,
            current,
            today,
            self.preferences.weather_mph,
            clock_12h,
        );

        let contents = if wide {
            split()
                .flex_none()
                .gap(r(18.))
                .child(col().flex_1().gap(r(12.)).child(summary).child(details))
                .child(col().flex_1().child(chart))
                .child(col().flex_1().child(days))
        } else if split_forecast {
            split()
                .flex_none()
                .gap(r(14.))
                .child(
                    col()
                        .flex_1()
                        .gap(r(9.))
                        .child(summary)
                        .child(chart)
                        .child(details),
                )
                .child(col().flex_1().child(days))
        } else {
            col()
                .flex_none()
                .gap(r(9.))
                .child(summary)
                .child(chart)
                .child(days)
                .child(details)
        };

        tile.child(contents)
            .children(
                state
                    .weather_error
                    .as_ref()
                    .map(|error| theme.quiet(error.clone())),
            )
            .children(state.weather_at.map(|at| {
                let minutes = at.elapsed().as_secs() / 60;
                theme
                    .quiet(format!("Open-Meteo / CAMS · {minutes} min ago"))
                    .text_size(r(10.))
                    .flex_none()
            }))
    }
}

fn summary(
    theme: Theme,
    name: &str,
    current: Option<&weather::Conditions>,
    today: Option<&weather::Day>,
    fahrenheit: bool,
    compact: bool,
) -> Div {
    let code = current.and_then(|reading| reading.weather_code);
    let daylight = current.and_then(|reading| reading.is_day) != Some(0);
    let value = current.and_then(|reading| reading.temperature_2m);
    let temp_size = if compact { 34. } else { 45. };

    row()
        .flex_none()
        .justify_between()
        .gap(r(6.))
        .child(
            col()
                .flex_1()
                .gap(r(2.))
                .child(theme.label(name.to_owned()).truncate())
                .child(
                    div()
                        .text_size(r(temp_size))
                        .line_height(r(temp_size + 3.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.color(INK))
                        .whitespace_nowrap()
                        .child(temperature(value, fahrenheit, true)),
                )
                .child(
                    theme
                        .label(weather::condition(code))
                        .text_color(theme.color(INK))
                        .truncate(),
                )
                .children(today.map(|day| {
                    theme
                        .quiet(format!(
                            "H {}   L {}",
                            temperature(day.high, fahrenheit, false),
                            temperature(day.low, fahrenheit, false)
                        ))
                        .whitespace_nowrap()
                })),
        )
        .child(condition_art(
            theme,
            code,
            daylight,
            if compact { 42. } else { 58. },
        ))
}

fn weather_details(
    theme: Theme,
    aqi: Option<f32>,
    current: Option<&weather::Conditions>,
    today: Option<&weather::Day>,
    mph: bool,
    clock_12h: bool,
) -> Div {
    let wind = current
        .and_then(|reading| reading.wind_speed_10m)
        .map(|speed| {
            if mph {
                format!("{:.0} mph", speed * 0.621_371)
            } else {
                format!("{speed:.0} km/h")
            }
        });
    let time = |time: jiff::civil::Time| {
        time.strftime(if clock_12h { "%-I:%M %p" } else { "%H:%M" })
            .to_string()
    };
    let air = aqi.map_or_else(
        || "Air quality unavailable".into(),
        |aqi| format!("US AQI {aqi:.0} · {}", weather::air_category(aqi)),
    );

    col()
        .flex_none()
        .gap(r(6.))
        .pt(r(8.))
        .border_t_1()
        .border_color(theme.color(TEXT).opacity(0.16))
        .child(
            split()
                .justify_between()
                .gap(r(4.))
                .child(detail(theme, "Wind", wind))
                .child(detail(
                    theme,
                    "Sunrise",
                    today.and_then(|day| day.sunrise).map(time),
                ))
                .child(detail(
                    theme,
                    "Sunset",
                    today.and_then(|day| day.sunset).map(time),
                )),
        )
        .child(theme.quiet(air))
}

fn detail(theme: Theme, name: &'static str, value: Option<String>) -> Div {
    col()
        .gap(r(1.))
        .child(theme.quiet(name).text_size(r(10.)).whitespace_nowrap())
        .child(
            theme
                .label(value.unwrap_or_else(|| "—".into()))
                .text_size(r(11.))
                .whitespace_nowrap(),
        )
}

fn daily_forecast(
    theme: Theme,
    forecast: Option<&weather::Weather>,
    fahrenheit: bool,
    compact: bool,
) -> Div {
    let list = col().flex_none().gap(r(1.));
    let Some(weather) = forecast.filter(|weather| !weather.days.is_empty()) else {
        return list.child(theme.quiet("Daily forecast unavailable"));
    };

    let (low, high) = weather
        .days
        .iter()
        .flat_map(|day| [day.low, day.high])
        .flatten()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), temp| {
            (low.min(temp), high.max(temp))
        });
    let span = (high - low).max(1.);
    let start = (low + high - span) * 0.5;
    let normalize = |temperature: f32| ((temperature - start) / span).clamp(0., 1.);
    let list = list.child(
        theme
            .label(format!("{}-day forecast", weather.days.len()))
            .text_color(theme.color(INK))
            .pb(r(3.)),
    );
    list.children(weather.days.iter().map(|day| {
        let today = Some(day.date) == weather.today;
        let name = if today {
            "Today".into()
        } else {
            day.date.strftime("%a %-d").to_string()
        };
        let description = format!(
            "{} · {}",
            day.date.strftime("%A, %b %e"),
            weather::condition(day.weather_code)
        );
        let (symbol, accent) = weather_symbol(day.weather_code, true);
        let track = div()
            .relative()
            .flex_1()
            .h(r(4.))
            .rounded_full()
            .bg(theme.color(TEXT).opacity(0.17));
        let range = day.low.zip(day.high).filter(|(low, high)| low <= high);
        let track = track.children(range.map(|(low, high)| {
            let length = (normalize(high) - normalize(low)).max(0.04);
            div()
                .absolute()
                .left(relative(normalize(low).min(1. - length)))
                .w(relative(length))
                .h_full()
                .rounded_full()
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(temperature_color(low), 0.),
                    linear_color_stop(temperature_color(high), 1.),
                ))
        }));

        row()
            .id(("weather-day", day.date.day_of_year() as usize))
            .flex_none()
            .h(r(if compact { 22. } else { 24. }))
            .gap(r(5.))
            .child(
                theme
                    .label(name)
                    .text_color(theme.color(INK))
                    .w(r(53.))
                    .flex_none()
                    .truncate(),
            )
            .child(icon(symbol, theme.color(accent), 16.))
            .child(daily_temperature(theme, day.low, fahrenheit, MUTED))
            .child(track)
            .child(daily_temperature(theme, day.high, fahrenheit, INK))
            .tooltip(move |_, cx| cx.new(|_| Tooltip(description.clone().into())).into())
    }))
}

fn daily_temperature(theme: Theme, value: Option<f32>, fahrenheit: bool, color: Color) -> Div {
    theme
        .label(temperature(value, fahrenheit, false))
        .w(r(31.))
        .flex_none()
        .text_size(r(11.))
        .text_color(theme.color(color))
        .text_right()
        .whitespace_nowrap()
}

fn temperature_color(celsius: f32) -> gpui::Hsla {
    let warmth = ((celsius + 15.) / 55.).clamp(0., 1.);
    hsla((230. - 220. * warmth) / 360., 0.71, 0.69, 1.)
}

fn condition_art(theme: Theme, code: Option<u8>, daylight: bool, size: f32) -> Div {
    let (symbol, accent) = weather_symbol(code, daylight);
    let art = row()
        .size(r(size))
        .flex_none()
        .justify_center()
        .rounded_full();
    if matches!(symbol, Icon::Sun) {
        return art.bg(theme.color(AMBER).opacity(0.09)).child(
            row()
                .size(r(size * 0.81))
                .justify_center()
                .rounded_full()
                .bg(theme.color(AMBER).opacity(0.15))
                .child(
                    div()
                        .size(r(size * 0.63))
                        .rounded_full()
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(rgb(0xffed9c), 0.),
                            linear_color_stop(rgb(0xfda74e), 1.),
                        )),
                ),
        );
    }
    art.bg(theme.color(accent).opacity(0.08))
        .child(icon(symbol, theme.color(accent), size * 0.7))
}

fn weather_symbol(code: Option<u8>, daylight: bool) -> (Icon, Color) {
    match code {
        Some(0) if !daylight => (Icon::Moon, BLUE),
        Some(0) => (Icon::Sun, AMBER),
        Some(1 | 2) if daylight => (Icon::PartlyCloudy, AMBER),
        Some(1..=3) => (Icon::Cloud, TEXT),
        Some(45 | 48) => (Icon::Fog, TEXT),
        Some(51..=67 | 80..=82) => (Icon::Rain, BLUE),
        Some(71..=77 | 85 | 86) => (Icon::Snow, SNOW),
        Some(95..=99) => (Icon::Storm, VIOLET),
        _ => (Icon::Cloud, MUTED),
    }
}
