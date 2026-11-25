use gpui::{
    App, Bounds, Div, Hsla, PathBuilder, Pixels, Point, Styled, TextRun, Window, canvas, div, fill,
    font, linear_color_stop, linear_gradient, point, prelude::*, px, size,
};
use jiff::civil::Time;

use super::weather::{HOURS_PER_DAY, Weather, temperature, temperature_unit};
use crate::ui::{Tooltip, theme::*};

const INSET: f32 = 16.;

pub(super) fn today_chart(
    theme: Theme,
    weather: Option<&Weather>,
    height: f32,
    fahrenheit: bool,
    clock_12h: bool,
) -> Div {
    let chart = col().flex_none().gap(r(3.)).child(
        row()
            .justify_between()
            .child(
                theme
                    .label("Today’s temperature")
                    .text_color(theme.color(INK))
                    .truncate(),
            )
            .child(theme.quiet(temperature_unit(fahrenheit)).flex_none()),
    );
    let Some(weather) = weather.filter(|weather| weather.has_hourly_temperatures()) else {
        return chart.child(theme.quiet("Hourly temperatures unavailable"));
    };
    let hourly = weather.hourly;
    let current_hour = weather.time.map(|time| time.hour() as usize);
    chart.child(
        div()
            .relative()
            .h(r(height))
            .child(
                canvas(
                    |_, _, _| (),
                    move |area, _, window, cx| {
                        let rem_size = window.rem_size();
                        let units = |value| rem_size * (value / 16.);
                        let inset = units(INSET);
                        let left = area.left() + inset;
                        let width = (area.size.width - inset * 2.).max(px(0.));
                        let top = area.top() + units(24.);
                        let bottom = area.bottom() - units(27.);
                        let x = |hour| left + width * ((hour as f32 + 0.5) / HOURS_PER_DAY as f32);
                        let (low, high) = hourly
                            .iter()
                            .flatten()
                            .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), &temp| {
                                (low.min(temp), high.max(temp))
                            });
                        let span = (high - low).max(2.);
                        let middle = (high + low) * 0.5;
                        let points = std::array::from_fn::<_, HOURS_PER_DAY, _>(|hour| {
                            hourly[hour].map(|temp| {
                                let y = 0.5 - (temp - middle) / span;
                                point(x(hour), top + (bottom - top) * y)
                            })
                        });

                        for y in [top, (top + bottom) * 0.5, bottom] {
                            window.paint_quad(fill(
                                Bounds::new(point(left, y), size(width, px(1.))),
                                theme.color(TEXT).opacity(0.14),
                            ));
                        }
                        if let Some(now) = current_hour {
                            window.paint_quad(fill(
                                Bounds::new(point(x(now), top), size(px(1.), bottom - top)),
                                theme.color(BLUE).opacity(0.48),
                            ));
                        }
                        for segment in points.split(Option::is_none) {
                            let mut segment = segment.iter().flatten().copied();
                            if let Some(first) = segment.next() {
                                paint_curve(theme, first, segment, bottom, units(1.65), window);
                            }
                        }
                        if let Some(now) = current_hour.and_then(|hour| points[hour]) {
                            let diameter = units(6.);
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        now - point(diameter * 0.5, diameter * 0.5),
                                        size(diameter, diameter),
                                    ),
                                    theme.color(BLUE),
                                )
                                .corner_radii(diameter * 0.5),
                            );
                        }

                        let ticks: &[usize] = if area.size.width >= units(315.) {
                            &[0, 4, 8, 12, 16, 20, 23]
                        } else {
                            &[0, 6, 12, 18, 23]
                        };
                        let font_size = units(11.);
                        let time_y = bottom + units(9.);
                        let label_offset = units(18.);
                        for &hour in ticks {
                            let time = hour_label(hour, clock_12h);
                            paint_label(
                                time,
                                point(x(hour), time_y),
                                font_size,
                                theme.color(TEXT),
                                window,
                                cx,
                            );
                            if let Some(pos) = points[hour] {
                                paint_label(
                                    temperature(hourly[hour], fahrenheit, false),
                                    point(pos.x, pos.y - label_offset),
                                    font_size,
                                    theme.color(INK),
                                    window,
                                    cx,
                                );
                            }
                        }
                    },
                )
                .size_full(),
            )
            .child(row().absolute().inset_0().px(r(INSET)).children(
                hourly.into_iter().enumerate().map(|(hour, value)| {
                    let cell = div().flex_1().h_full().id(("weather-hour", hour));
                    if value.is_some() {
                        let reading = format!(
                            "{} · {}",
                            hour_label(hour, clock_12h),
                            temperature(value, fahrenheit, true)
                        );
                        cell.tooltip(move |_, cx| {
                            cx.new(|_| Tooltip(reading.clone().into())).into()
                        })
                    } else {
                        cell
                    }
                }),
            )),
    )
}

fn paint_curve(
    theme: Theme,
    first: Point<Pixels>,
    rest: impl Iterator<Item = Point<Pixels>>,
    bottom: Pixels,
    thickness: Pixels,
    window: &mut Window,
) {
    let mut line = PathBuilder::stroke(thickness);
    let mut area = PathBuilder::fill();
    line.move_to(first);
    area.move_to(first);
    let mut last = first;
    for next in rest {
        let step = (next.x - last.x) / 3.;
        let from = point(last.x + step, last.y);
        let to = point(next.x - step, next.y);
        line.cubic_bezier_to(next, from, to);
        area.cubic_bezier_to(next, from, to);
        last = next;
    }
    if last == first {
        let diameter = thickness * 2.;
        window.paint_quad(
            fill(
                Bounds::new(
                    first - point(thickness, thickness),
                    size(diameter, diameter),
                ),
                theme.color(INK),
            )
            .corner_radii(thickness),
        );
        return;
    }
    area.line_to(point(last.x, bottom));
    area.line_to(point(first.x, bottom));
    area.close();
    if let Ok(path) = area.build() {
        window.paint_path(
            path,
            linear_gradient(
                180.,
                linear_color_stop(theme.color(BLUE).opacity(0.22), 0.),
                linear_color_stop(theme.color(BLUE).opacity(0.03), 1.),
            ),
        );
    }
    if let Ok(path) = line.build() {
        window.paint_path(path, theme.color(INK));
    }
}

fn hour_label(hour: usize, clock_12h: bool) -> String {
    Time::new(hour as i8, 0, 0, 0)
        .expect("hour of the day")
        .strftime(if clock_12h { "%-I %p" } else { "%H:%M" })
        .to_string()
}

fn paint_label(
    text: String,
    position: Point<Pixels>,
    font_size: Pixels,
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let len = text.len();
    let line = window.text_system().shape_line(
        text.into(),
        font_size,
        &[TextRun {
            len,
            font: font("Inter"),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    let origin = point(position.x - line.width / 2., position.y);
    let line_height = font_size * 1.3;
    let _ = line.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx);
}
