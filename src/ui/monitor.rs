use gpui::{IntoElement, SharedString, Styled, div, prelude::*};

use crate::model::DiskUsage;

use super::{Tooltip, percent, theme::*};

mod chart;
pub(super) use chart::{discard_charts, graph};

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
    if bytes >= 1024. * 1024. * 1024. {
        (format!("{:.1}", bytes / (1024. * 1024. * 1024.)), "GiB/s")
    } else if bytes >= 1024. * 1024. {
        (format!("{:.1}", bytes / (1024. * 1024.)), "MiB/s")
    } else if bytes >= 1024. {
        (format!("{:.1}", bytes / 1024.), "KiB/s")
    } else {
        (format!("{bytes:.0}"), "B/s")
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
