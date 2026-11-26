use std::collections::VecDeque;

use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, SharedString, Styled, div, prelude::*,
};

use crate::model::{DiskUsage, Sensor};

use super::{
    super::{
        Dashboard, Tooltip,
        icons::Icon,
        monitor::{capacity, drive_load, gauge, gib, monitoring_bar, rate, uptime},
        theme::*,
    },
    options::{ChartLayout, MAX_PLOTS, MonitorDisplay, MonitorOptions, MonitorSource},
};

mod layout;
mod plots;
mod setup;
pub(super) use setup::MonitorSetup;

const GAP: f32 = 8.;

struct Card<'a> {
    readings: Vec<Reading<'a>>,
    display: MonitorDisplay,
    layout: ChartLayout,
}

impl Card<'_> {
    fn width(&self) -> f32 {
        match self.display {
            MonitorDisplay::Bar => 128.,
            MonitorDisplay::Gauge => 136.,
            MonitorDisplay::Chart
                if self.layout == ChartLayout::Separate && self.readings.len() > 1 =>
            {
                16. + self.readings.len() as f32 * 108. + (self.readings.len() - 1) as f32 * GAP
            }
            MonitorDisplay::Chart if self.readings.len() > 1 => 232.,
            MonitorDisplay::Chart => 168.,
        }
    }

    fn height(&self, width: f32) -> f32 {
        let detail = if self.readings[0].detail.is_some() {
            17.
        } else {
            0.
        };
        match self.display {
            MonitorDisplay::Bar => 42. + detail,
            MonitorDisplay::Gauge => 60. + detail,
            MonitorDisplay::Chart => plots::chart_height(self, width),
        }
    }
}

struct Reading<'a> {
    name: &'a str,
    value: Option<f32>,
    unit: &'static str,
    samples: Option<&'a VecDeque<f32>>,
    detail: Option<String>,
    tooltip_detail: Option<String>,
    maximum: Option<f32>,
}

impl Dashboard {
    pub(super) fn system_monitor_widget(
        &self,
        id: u32,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let defaults;
        let options = if let Some(saved) = self.home.layout().monitors.get(&id) {
            saved
        } else {
            defaults = MonitorOptions::default();
            &defaults
        };
        if let Some(setup) = &self.home.monitor_setup
            && setup.id == id
        {
            return self
                .monitor_setup_menu(setup, options, cx)
                .into_any_element();
        }

        let mut cards = Vec::new();
        for selected in &options.readings {
            if selected.source == MonitorSource::VisibleDrives {
                cards.extend(self.visible_disks().into_iter().filter_map(|disk| {
                    let individually_selected = options.readings.iter().any(|item| {
                        let comparisons = item
                            .chart
                            .additional
                            .iter()
                            .filter(|_| item.display == MonitorDisplay::Chart);
                        std::iter::once(&item.source).chain(comparisons).any(
                            |source| matches!(source, MonitorSource::Drive(id) if id == &disk.id),
                        )
                    });
                    (!individually_selected).then(|| Card {
                        readings: vec![self.drive_reading(disk)],
                        display: selected.display,
                        layout: selected.chart.layout,
                    })
                }));
            } else {
                let comparisons = selected
                    .chart
                    .additional
                    .iter()
                    .filter(|_| selected.display == MonitorDisplay::Chart)
                    .take(MAX_PLOTS - 1);
                let readings = std::iter::once(&selected.source)
                    .chain(comparisons)
                    .filter_map(|source| self.monitor_reading(source))
                    .collect::<Vec<_>>();
                if !readings.is_empty() {
                    cards.push(Card {
                        readings,
                        display: selected.display,
                        layout: selected.chart.layout,
                    });
                }
            }
        }
        let single = cards.len() == 1;
        let padding = if single {
            0.
        } else {
            super::widget_padding(height + 2.)
        };
        let width = (width - 2. * padding).max(1.);
        let height = height - 2. * padding;
        let wide = width >= 408.;
        let header_height = if wide { 32. } else { 44. };
        let widget = col()
            .flex_1()
            .p(r(padding))
            .gap(r(GAP))
            .when(options.show_top_bar, |it| {
                it.child(
                    self.monitor_top_bar(theme, width, header_height, wide)
                        .when(single, |header| header.mx(r(GAP)).mt(r(GAP))),
                )
            });
        let readings = col()
            .id(("monitor-readings", id as usize))
            .flex_1()
            .overflow_y_scroll();
        let header = if options.show_top_bar {
            header_height + GAP + if single { GAP } else { 0. }
        } else {
            0.
        };
        if let [card] = cards.as_slice() {
            let content = match card.display {
                MonitorDisplay::Chart => plots::chart_card((id, 0), theme, card, width, 0., true),
                display => meter_card(
                    0,
                    theme,
                    &card.readings[0],
                    display,
                    Some((width, height - header)),
                ),
            };
            return widget.child(readings.child(content)).into_any_element();
        }
        let mut positions = layout::place_cards(
            width,
            cards.iter().map(|card| {
                let width = card.width().min(width);
                (width, card.height(width))
            }),
        );
        // Charts can grow into free space after the smaller cards have found their positions.
        for (index, card) in cards.iter().enumerate() {
            if card.display != MonitorDisplay::Chart {
                continue;
            }
            let position = positions[index];
            let right = positions
                .iter()
                .copied()
                .filter(|other| {
                    other.x >= position.right() && other.with_gap().shares_rows(position.with_gap())
                })
                .map(|other| other.x - GAP)
                .fold(width, f32::min);
            positions[index].width = right - position.x;
        }
        for (index, card) in cards.iter().enumerate() {
            if card.display != MonitorDisplay::Chart {
                continue;
            }
            let position = positions[index];
            let bottom = positions
                .iter()
                .copied()
                .filter(|other| {
                    other.y >= position.bottom()
                        && other.with_gap().shares_columns(position.with_gap())
                })
                .map(|other| other.y - GAP)
                .fold(height - header, f32::min);
            positions[index].height += (bottom - position.bottom()).clamp(0., 64.);
        }
        let content_height = positions
            .iter()
            .map(|position| position.bottom())
            .fold(0., f32::max);
        widget
            .child(if cards.is_empty() {
                readings
                    .gap(r(GAP))
                    .when(options.readings.is_empty() && self.home.editing, |it| {
                        it.child(theme.quiet("No cards yet."))
                    })
                    .when(!options.readings.is_empty(), |it| {
                        it.child(theme.quiet("No visible drives."))
                    })
                    .when(!self.home.editing, |it| {
                        it.child(row().child(self.monitor_add_cards_button(id, cx)))
                    })
            } else {
                readings.child(
                    div()
                        .relative()
                        .w_full()
                        .h(r(content_height))
                        .flex_none()
                        .children(cards.iter().zip(positions).enumerate().map(
                            |(index, (card, position))| {
                                let element = match card.display {
                                    MonitorDisplay::Chart => plots::chart_card(
                                        (id, index),
                                        theme,
                                        card,
                                        position.width,
                                        (position.height - card.height(position.width)).max(0.),
                                        false,
                                    ),
                                    display => {
                                        meter_card(index, theme, &card.readings[0], display, None)
                                    }
                                };
                                element
                                    .absolute()
                                    .left(r(position.x))
                                    .top(r(position.y))
                                    .w(r(position.width))
                                    .h(r(position.height))
                            },
                        )),
                )
            })
            .into_any_element()
    }

    fn sensor(&self, id: &str) -> Option<&Sensor> {
        if id == "gpu" {
            return self
                .metrics
                .sensors
                .iter()
                .filter(|sensor| sensor.id.starts_with("gpu:") && sensor.id.ends_with(":load"))
                .max_by_key(|sensor| sensor.value.is_some());
        }
        self.metrics.sensors.iter().find(|sensor| sensor.id == id)
    }

    fn monitor_source_name<'a>(&'a self, source: &'a MonitorSource) -> &'a str {
        match source {
            MonitorSource::Sensor(key) => self.sensor(key).map_or_else(
                || match key.as_str() {
                    "cpu" => "CPU load",
                    "gpu" => "GPU load",
                    "memory" => "Memory load",
                    "ram-used" => "Memory used",
                    key => key,
                },
                |sensor| &sensor.name,
            ),
            MonitorSource::Download => "Download",
            MonitorSource::Upload => "Upload",
            MonitorSource::Drive(key) => self
                .storage
                .iter()
                .find(|disk| &disk.id == key)
                .map_or(key, |disk| &disk.label),
            MonitorSource::VisibleDrives => "All visible drives",
        }
    }

    fn monitor_reading<'a>(&'a self, source: &'a MonitorSource) -> Option<Reading<'a>> {
        let name = self.monitor_source_name(source);
        let (value, unit, samples, detail, tooltip_detail, maximum) = match source {
            MonitorSource::Sensor(key) => {
                let sensor = self.sensor(key);
                let ram = matches!(key.as_str(), "memory" | "ram-used");
                let tooltip_detail = (ram && self.metrics.memory_total > 0).then(|| {
                    format!(
                        "{:.1} of {:.1} GiB used",
                        gib(self.metrics.memory_used),
                        gib(self.metrics.memory_total),
                    )
                });
                let maximum = (key == "ram-used" && self.metrics.memory_total > 0)
                    .then(|| gib(self.metrics.memory_total) as f32);
                (
                    sensor.and_then(|value| value.value),
                    sensor.map_or(if key == "gpu" { "%" } else { "" }, |sensor| sensor.unit),
                    sensor.and_then(|sensor| self.history.sensors.get(&sensor.id)),
                    None,
                    tooltip_detail,
                    maximum,
                )
            }
            MonitorSource::Download => (
                self.metrics.network.map(|rate| rate.download),
                "B/s",
                Some(&self.history.download),
                None,
                None,
                None,
            ),
            MonitorSource::Upload => (
                self.metrics.network.map(|rate| rate.upload),
                "B/s",
                Some(&self.history.upload),
                None,
                None,
                None,
            ),
            MonitorSource::Drive(key) => {
                if self.preferences.hidden_drives.contains_key(key) {
                    return None;
                }
                if let Some(disk) = self.storage.iter().find(|disk| &disk.id == key) {
                    return Some(self.drive_reading(disk));
                }
                (
                    None,
                    "%",
                    None,
                    Some("Drive unavailable".into()),
                    None,
                    None,
                )
            }
            MonitorSource::VisibleDrives => return None,
        };
        Some(Reading {
            name,
            value,
            unit,
            samples,
            detail,
            tooltip_detail,
            maximum,
        })
    }

    fn drive_reading<'a>(&'a self, disk: &'a DiskUsage) -> Reading<'a> {
        let total = capacity(disk.total);
        let load = drive_load(disk);
        Reading {
            name: &disk.label,
            value: load,
            unit: "%",
            samples: self.history.drives.get(&disk.id),
            detail: Some(match disk.used.filter(|_| load.is_some()) {
                Some(used) => format!("{} used · {total} total", capacity(used)),
                None => format!("{total} total · Usage unavailable"),
            }),
            tooltip_detail: None,
            maximum: None,
        }
    }

    fn monitor_top_bar(&self, theme: Theme, width: f32, height: f32, wide: bool) -> Div {
        let host = self
            .metrics
            .uptime
            .map(|seconds| format!("{} · {}", self.metrics.host, uptime(seconds)));
        let label = |text| {
            theme
                .quiet(text)
                .text_size(r(10.))
                .line_height(r(13.))
                .truncate()
        };
        row()
            .h(r(height))
            .flex_none()
            .gap(r(8.))
            .child(theme.icon_tile(Icon::Gauge, TEAL, 28.))
            .child(
                col()
                    .flex_1()
                    .child(
                        theme
                            .label("System monitor")
                            .text_color(theme.color(INK))
                            .text_size(r(15.))
                            .line_height(r(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate(),
                    )
                    .when(!self.metrics.cpu_name.is_empty(), |it| {
                        it.child(label(self.metrics.cpu_name.clone()))
                    })
                    .children(host.as_ref().filter(|_| !wide).cloned().map(label)),
            )
            .children(
                host.filter(|_| wide)
                    .map(|text| label(text).max_w(r(width * 0.4))),
            )
    }
}

impl Reading<'_> {
    fn range(&self) -> (f32, f32) {
        if self.unit == "%" {
            return (0., 100.);
        }
        if let Some(maximum) = self.maximum {
            return (0., maximum);
        }
        let minimum_peak: f32 = if self.unit == "B/s" { 1024. } else { 1. };
        self.samples
            .into_iter()
            .flatten()
            .copied()
            .chain(self.value)
            .fold((0f32, minimum_peak), |(low, high), n| {
                (low.min(n), high.max(n))
            })
    }
}

fn card_frame(index: usize, theme: Theme, accent: Color, full: bool) -> gpui::Stateful<Div> {
    let frame = col()
        .id(("monitor-reading", index))
        .overflow_hidden()
        .gap(r(4.));
    if full {
        frame.flex_1()
    } else {
        frame
            .flex_none()
            .p(r(7.))
            .rounded(r(6.))
            .border_1()
            .border_color(theme.color(accent).opacity(0.24))
            .bg(theme.color(BACK).opacity(0.48))
    }
}

impl Reading<'_> {
    fn value_text(&self) -> String {
        self.value
            .map_or_else(|| "—".into(), |n| format_value(n, self.unit))
    }

    fn tooltip(&self, (low, high): (f32, f32)) -> SharedString {
        let mut label = format!("{} · {}", self.name, self.value_text());
        if let Some(detail) = self.detail.as_ref().or(self.tooltip_detail.as_ref()) {
            label.push_str(&format!("\n{detail}"));
        }
        if self.value.is_some() || self.samples.is_some_and(|samples| !samples.is_empty()) {
            label.push_str(&format!(
                "\nScale {} to {}",
                format_value(low, self.unit),
                format_value(high, self.unit)
            ));
        }
        label.into()
    }
}

fn meter_card(
    index: usize,
    theme: Theme,
    reading: &Reading<'_>,
    display: MonitorDisplay,
    size: Option<(f32, f32)>,
) -> gpui::Stateful<Div> {
    let full = size.is_some();
    let accent = if display == MonitorDisplay::Gauge {
        VIOLET
    } else {
        BLUE
    };
    let range = reading.range();
    let normalized = reading
        .value
        .map(|n| (n - range.0) / (range.1 - range.0) * 100.);
    let tooltip = reading.tooltip(range);
    let value = reading.value_text();
    let (number, unit) = if display == MonitorDisplay::Gauge {
        value
            .split_once(' ')
            .map_or((value.as_str(), None), |(number, unit)| {
                (number, Some(unit))
            })
    } else {
        (value.as_str(), None)
    };
    let name = theme
        .label(reading.name.to_owned())
        .text_size(r(if full { 15. } else { 11. }))
        .line_height(r(if full { 20. } else { 15. }))
        .truncate();
    let value = mono(number.to_owned())
        .text_size(r(if full { 26. } else { 13. }))
        .line_height(r(if full { 34. } else { 17. }))
        .text_color(theme.color(accent))
        .truncate();
    let card = card_frame(index, theme, accent, full)
        .tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip.clone())).into());
    let card = if display == MonitorDisplay::Gauge {
        let diameter = size.map_or(43., |(width, height)| {
            (width * 0.53).min(height - 32.).max(32.)
        });
        card.child(
            row()
                .gap(r(5.))
                .when(full, |row| row.flex_1().gap(r(12.)).px(r(GAP)))
                .child(gauge(theme, normalized, accent, diameter))
                .child(
                    col()
                        .flex_1()
                        .child(name)
                        .child(value)
                        .children(unit.map(|unit| {
                            theme
                                .quiet(unit.to_owned())
                                .text_size(r(10.))
                                .line_height(r(12.))
                                .truncate()
                        })),
                ),
        )
    } else {
        card.when(full, |card| card.justify_center().gap(r(GAP)))
            .child(
                row()
                    .justify_between()
                    .gap(r(3.))
                    .when(full, |row| row.px(r(GAP)))
                    .child(name.flex_1())
                    .child(value.flex_none()),
            )
            .child(monitoring_bar(
                theme,
                normalized.unwrap_or_default(),
                accent,
                size.map_or(5., |(_, height)| (height * 0.1).clamp(6., 24.)),
            ))
    };
    card.children(reading.detail.as_ref().map(|detail| {
        reading_detail(theme, detail).when(full, |detail| detail.px(r(GAP)).pb(r(4.)))
    }))
}

fn reading_detail(theme: Theme, detail: &str) -> Div {
    theme
        .quiet(detail.to_owned())
        .text_size(r(10.))
        .line_height(r(13.))
        .flex_none()
        .truncate()
}

fn format_value(value: f32, unit: &str) -> String {
    match unit {
        "B/s" => {
            let (number, unit) = rate(value);
            format!("{number} {unit}")
        }
        "%" => format!("{value:.0}%"),
        "V" | "A" => format!("{value:.2} {unit}"),
        "GiB" | "W" | "°C" => format!("{value:.1} {unit}"),
        "" => format!("{value:.0}"),
        _ => format!("{value:.0} {unit}"),
    }
}
