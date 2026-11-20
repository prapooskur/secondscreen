use std::collections::VecDeque;

use gpui::{
    Bounds, Div, FontWeight, IntoElement, PathBuilder, SharedString, Styled, canvas, div, fill,
    linear_color_stop, linear_gradient, point, prelude::*, px, size,
};

use crate::model::DiskUsage;

use super::{
    Dashboard, HISTORY, Tooltip,
    icons::{Icon, icon},
    percent,
    theme::*,
};

impl Dashboard {
    pub(super) fn system_page(&self) -> impl IntoElement {
        let metrics = &self.metrics;
        let gpu = metrics.gpu.as_ref();
        let memory = percent(metrics.memory_used, metrics.memory_total);
        let temperature = metrics.temperature.map(|value| format!("{value:.0} °C"));
        let network_top = self
            .history
            .download
            .iter()
            .chain(self.history.upload.iter())
            .copied()
            .fold(65_536.0, f32::max)
            * 1.08;
        let (download, unit) = rate(metrics.download);

        let cpu = card(
            "Processor",
            Icon::Cpu,
            TEAL,
            reading(metrics.cpu.map(|value| format!("{value:.0}")), "%"),
            graph(self.history.cpu.clone(), None, 100., TEAL),
            quiet(if metrics.cpu_name.is_empty() {
                "CPU data unavailable".into()
            } else {
                metrics.cpu_name.clone()
            })
            .truncate(),
            temperature
                .or_else(|| (metrics.threads > 0).then(|| format!("{} threads", metrics.threads))),
        );
        let graphics = card(
            "Graphics",
            Icon::Gpu,
            BLUE,
            reading(
                gpu.and_then(|gpu| gpu.load)
                    .map(|load| format!("{load:.0}")),
                "%",
            ),
            graph(self.history.gpu.clone(), None, 100., BLUE),
            quiet(
                gpu.map(|gpu| gpu.name.clone())
                    .unwrap_or_else(|| "GPU metrics unavailable".into()),
            )
            .truncate(),
            gpu.and_then(|gpu| gpu.vram)
                .map(|(used, total)| format!("{:.1}/{:.0} GiB", gib(used), gib(total))),
        );
        let memory_card = card(
            "Memory",
            Icon::Memory,
            VIOLET,
            reading(memory.map(|value| format!("{value:.0}")), "%"),
            graph(self.history.memory.clone(), None, 100., VIOLET),
            quiet(memory.map_or_else(
                || "Memory data unavailable".to_owned(),
                |_| {
                    format!(
                        "{:.1} of {:.1} GiB used",
                        gib(metrics.memory_used),
                        gib(metrics.memory_total)
                    )
                },
            ))
            .truncate(),
            None,
        );
        let network = card(
            "Network",
            Icon::Network,
            AMBER,
            reading(metrics.network_available.then_some(download), unit),
            graph(
                self.history.download.clone(),
                Some(self.history.upload.clone()),
                network_top,
                AMBER,
            ),
            if metrics.network_available {
                let (upload, unit) = rate(metrics.upload);
                row()
                    .gap(r(5.))
                    .child(icon(Icon::Up, c(MUTED), 12.))
                    .child(quiet(format!("{upload} {unit} upload")).truncate())
            } else {
                quiet("No active network interface").truncate()
            },
            metrics.network_available.then(|| "Download".to_owned()),
        );

        col()
            .flex_1()
            .gap(r(12.))
            .child(
                row()
                    .justify_between()
                    .gap(r(8.))
                    .flex_none()
                    .child(
                        col()
                            .flex_1()
                            .gap(r(4.))
                            .child(title("System monitor"))
                            .child(
                                row()
                                    .gap(r(16.))
                                    .child(
                                        quiet(if metrics.host.is_empty() {
                                            "Sampling this system…".to_owned()
                                        } else {
                                            metrics.host.clone()
                                        })
                                        .max_w(r(220.))
                                        .truncate(),
                                    )
                                    .child(detail(
                                        Icon::Clock,
                                        "Uptime",
                                        metrics.uptime.map_or_else(|| "—".into(), uptime),
                                    )),
                            ),
                    )
                    .child(
                        row()
                            .gap(r(9.))
                            .flex_none()
                            .child(quiet("Last 60 seconds"))
                            .child(chip(
                                row()
                                    .gap(r(6.))
                                    .child(dot(if metrics.host.is_empty() { AMBER } else { TEAL }))
                                    .child(if metrics.host.is_empty() {
                                        "Starting"
                                    } else {
                                        "Live"
                                    }),
                                if metrics.host.is_empty() { AMBER } else { TEAL },
                            )),
                    ),
            )
            .child(
                split()
                    .flex_1()
                    .gap(r(12.))
                    .min_h_0()
                    .child(cpu)
                    .child(graphics)
                    .child(memory_card)
                    .child(network)
                    .child(
                        panel()
                            .w(r(218.))
                            .flex_none()
                            .overflow_hidden()
                            .p(r(12.))
                            .gap(r(9.))
                            .child(
                                row()
                                    .flex_none()
                                    .gap(r(8.))
                                    .child(icon_tile(Icon::Disk, BLUE, 30.))
                                    .child(label("Storage")),
                            )
                            .child(storage_list("system-storage", self.visible_disks())),
                    ),
            )
    }
}

pub(super) fn storage_list(id: &'static str, disks: Vec<&DiskUsage>) -> impl IntoElement {
    col()
        .id(id)
        .flex_1()
        .overflow_y_scroll()
        .gap(r(11.))
        .when(disks.is_empty(), |it| {
            it.child(quiet("No mounted volumes shown."))
        })
        .children(disks.into_iter().map(|disk| {
            let load = percent(disk.used, disk.total).unwrap_or_default();
            let mount = SharedString::from(disk.mount.clone());
            col()
                .flex_none()
                .gap(r(4.))
                .child(
                    row()
                        .gap(r(6.))
                        .justify_between()
                        .child(
                            label(mount.clone())
                                .id(mount.clone())
                                .flex_1()
                                .text_color(c(INK))
                                .truncate()
                                .tooltip(move |_, cx| cx.new(|_| Tooltip(mount.clone())).into()),
                        )
                        .child(
                            mono(format!("{load:.0}%"))
                                .flex_none()
                                .text_size(r(11.))
                                .text_color(c(BLUE)),
                        ),
                )
                .child(
                    quiet(format!(
                        "{} of {} used",
                        capacity(disk.used),
                        capacity(disk.total)
                    ))
                    .whitespace_nowrap(),
                )
                .child(
                    div().h(r(4.)).rounded_full().bg(c(STROKE)).child(
                        div()
                            .w(gpui::relative((load / 100.).clamp(0., 1.)))
                            .h_full()
                            .rounded_full()
                            .bg(c(BLUE)),
                    ),
                )
        }))
}

fn detail(symbol: Icon, heading: &str, value: String) -> Div {
    row()
        .gap(r(8.))
        .child(icon(symbol, c(MUTED), 15.))
        .child(quiet(heading.to_owned()).whitespace_nowrap())
        .child(mono(value).text_size(r(12.)).text_color(c(INK)).truncate())
}

fn reading(value: Option<String>, unit: &str) -> Div {
    let available = value.is_some();
    row()
        .items_baseline()
        .gap(r(5.))
        .flex_none()
        .overflow_hidden()
        .child(
            mono(value.unwrap_or_else(|| "—".into()))
                .text_size(r(34.))
                .line_height(r(43.)),
        )
        .when(available, |it| {
            it.child(
                mono(unit.to_owned())
                    .text_size(r(13.))
                    .text_color(c(MUTED))
                    .truncate(),
            )
        })
}

fn card(
    name: &str,
    symbol: Icon,
    accent: u32,
    reading: Div,
    graph: impl IntoElement,
    detail: Div,
    extra: Option<String>,
) -> Div {
    panel()
        .flex_1()
        .overflow_hidden()
        .child(
            col()
                .flex_1()
                .gap(r(7.))
                .px(r(14.))
                .pt(r(12.))
                .pb(r(8.))
                .child(
                    row()
                        .gap(r(8.))
                        .flex_none()
                        .child(icon_tile(symbol, accent, 30.))
                        .child(
                            label(name.to_owned())
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate(),
                        ),
                )
                .child(reading)
                .child(graph),
        )
        .child(
            row()
                .h(r(34.))
                .px(r(14.))
                .gap(r(8.))
                .flex_none()
                .border_t_1()
                .border_color(c(STROKE))
                .child(detail.flex_1().min_w_0())
                .when_some(extra, |it, text| {
                    it.child(
                        mono(text)
                            .text_size(r(11.))
                            .text_color(c(accent))
                            .flex_none(),
                    )
                }),
        )
}

pub(super) fn graph(
    primary: VecDeque<f32>,
    secondary: Option<VecDeque<f32>>,
    maximum: f32,
    accent: u32,
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
                    c(STROKE),
                ));
            }
            let draw =
                |values: &VecDeque<f32>, color: u32, area_fill: bool, window: &mut gpui::Window| {
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
                                        linear_color_stop(c(color).opacity(0.23), 0.),
                                        linear_color_stop(c(color).opacity(0.025), 1.),
                                    ),
                                );
                            }
                        }
                        if first != last
                            && let Ok(path) = line.build()
                        {
                            window.paint_path(path, c(color));
                        }
                        let dot = Bounds::new(last - point(px(2.5), px(2.5)), size(px(5.), px(5.)));
                        window.paint_quad(fill(dot, c(color)).corner_radii(px(2.5)));
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
