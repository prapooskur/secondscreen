use std::collections::VecDeque;

use gpui::{
    Bounds, IntoElement, PathBuilder, SharedString, Styled, canvas, div, fill, linear_color_stop,
    linear_gradient, point, prelude::*, px, size,
};

use crate::model::DiskUsage;

use super::{HISTORY, Tooltip, percent, theme::*};

pub(super) fn storage_list(
    theme: Theme,
    id: &'static str,
    disks: Vec<&DiskUsage>,
) -> impl IntoElement {
    col()
        .id(id)
        .flex_1()
        .overflow_y_scroll()
        .gap(r(11.))
        .when(disks.is_empty(), |it| {
            it.child(theme.quiet("No drives shown."))
        })
        .children(disks.into_iter().map(|disk| {
            let load = disk
                .used
                .filter(|&used| used <= disk.total)
                .and_then(|used| percent(used, disk.total));
            let total = capacity(disk.total);
            let detail = disk.used.map_or_else(
                || format!("{total} total · Usage unavailable"),
                |used| format!("{} used · {total} total", capacity(used)),
            );
            let name = SharedString::from(disk.label.clone());
            let tooltip = SharedString::from(if disk.label == disk.device {
                disk.device.clone()
            } else {
                format!("{} · {}", disk.label, disk.device)
            });
            col()
                .flex_none()
                .gap(r(4.))
                .child(
                    row()
                        .gap(r(6.))
                        .justify_between()
                        .child(
                            theme
                                .label(name)
                                .id(SharedString::from(disk.id.clone()))
                                .flex_1()
                                .text_color(theme.color(INK))
                                .truncate()
                                .tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip.clone())).into()),
                        )
                        .when_some(load, |it, load| {
                            it.child(
                                mono(format!("{load:.0}%"))
                                    .flex_none()
                                    .text_size(r(11.))
                                    .text_color(theme.color(BLUE)),
                            )
                        }),
                )
                .child(theme.quiet(detail))
                .when_some(load, |it, load| {
                    it.child(
                        div().h(r(4.)).rounded_full().bg(theme.color(STROKE)).child(
                            div()
                                .w(gpui::relative(load / 100.))
                                .h_full()
                                .rounded_full()
                                .bg(theme.color(BLUE)),
                        ),
                    )
                })
        }))
}

pub(super) fn graph(
    theme: Theme,
    primary: VecDeque<f32>,
    secondary: Option<VecDeque<f32>>,
    maximum: f32,
    accent: Color,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |area, _, window, _| {
            let left = area.origin.x;
            let top = area.origin.y + px(3.);
            let bottom = (area.bottom() - px(4.)).max(top);
            let width = (area.size.width - px(4.)).max(px(0.));
            let height = bottom - top;
            for offset in [0., 0.5, 1.] {
                window.paint_quad(fill(
                    Bounds::new(point(left, top + height * offset), size(width, px(1.))),
                    theme.color(STROKE),
                ));
            }
            let draw = |values: &VecDeque<f32>,
                        color: Color,
                        area_fill: bool,
                        window: &mut gpui::Window| {
                let mut points = values.iter().enumerate().map(|(index, value)| {
                    let from_end = values.len() - index - 1;
                    let x = left + width * (1. - from_end as f32 / (HISTORY - 1) as f32);
                    let y = bottom - height * (value / maximum).clamp(0., 1.);
                    point(x, y)
                });
                if let Some(first) = points.next() {
                    let mut line = PathBuilder::stroke(px(1.65));
                    let mut fill_path = PathBuilder::fill();
                    line.move_to(first);
                    fill_path.move_to(first);
                    let mut last = first;
                    for next in points {
                        line.line_to(next);
                        fill_path.line_to(next);
                        last = next;
                    }
                    if area_fill && first != last {
                        fill_path.line_to(point(last.x, bottom));
                        fill_path.line_to(point(first.x, bottom));
                        fill_path.close();
                        if let Ok(path) = fill_path.build() {
                            window.paint_path(
                                path,
                                linear_gradient(
                                    180.,
                                    linear_color_stop(theme.color(color).opacity(0.23), 0.),
                                    linear_color_stop(theme.color(color).opacity(0.025), 1.),
                                ),
                            );
                        }
                    }
                    if first != last
                        && let Ok(path) = line.build()
                    {
                        window.paint_path(path, theme.color(color));
                    }
                    let dot = Bounds::new(last - point(px(2.5), px(2.5)), size(px(5.), px(5.)));
                    window.paint_quad(fill(dot, theme.color(color)).corner_radii(px(2.5)));
                }
            };
            if let Some(secondary) = &secondary {
                draw(secondary, BLUE, false, window);
            }
            draw(&primary, accent, true, window);
        },
    )
    .flex_1()
    .w_full()
    .min_h(r(14.))
}

pub(super) fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1024. * 1024. * 1024.)
}

pub(super) fn capacity(bytes: u64) -> String {
    let gib = gib(bytes);
    if gib >= 1024. {
        format!("{:.1} TiB", gib / 1024.)
    } else if gib >= 1. {
        format!("{gib:.0} GiB")
    } else {
        format!("{:.0} MiB", bytes as f64 / (1024. * 1024.))
    }
}

pub(super) fn rate(bytes: f32) -> (String, &'static str) {
    if bytes >= 1024. * 1024. {
        (format!("{:.1}", bytes / (1024. * 1024.)), "MiB/s")
    } else {
        (format!("{:.1}", bytes / 1024.), "KiB/s")
    }
}

pub(super) fn uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds / 3600 % 24;
    let minutes = seconds / 60 % 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else {
        format!("{hours}h {minutes}m")
    }
}
