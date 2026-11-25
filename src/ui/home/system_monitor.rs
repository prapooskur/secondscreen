use std::{collections::VecDeque, ops::Range};

use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, SharedString, Styled, prelude::*};

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
}

fn card_rows(width: f32, sizes: impl IntoIterator<Item = f32>) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut end = 0;
    let mut used = 0.;
    for size in sizes {
        let size = size.min(width);
        if end > start && used + GAP + size > width {
            rows.push(start..end);
            start = end;
            used = 0.;
        }
        used += if end > start { GAP + size } else { size };
        end += 1;
    }
    if end > start {
        rows.push(start..end);
    }
    rows
}

struct Reading<'a> {
    name: &'a str,
    value: Option<f32>,
    unit: &'static str,
    samples: Option<&'a VecDeque<f32>>,
    detail: Option<String>,
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
                .monitor_setup_menu(setup, options, width, cx)
                .into_any_element();
        }

        let wide = width >= 408.;
        let header_height = if wide { 32. } else { 44. };
        let width = width.max(1.);
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
        let rows = card_rows(width, cards.iter().map(Card::width));
        let header = if options.show_top_bar {
            header_height + GAP
        } else {
            0.
        };
        let (minimum_height, chart_rows) = rows.iter().fold((0., 0), |(height, charts), row| {
            let row_cards = &cards[row.clone()];
            if row_cards
                .iter()
                .any(|card| card.display == MonitorDisplay::Chart)
            {
                (height + 130., charts + 1)
            } else if row_cards
                .iter()
                .any(|card| card.display == MonitorDisplay::Gauge)
            {
                (height + 76., charts)
            } else {
                (height + 54., charts)
            }
        });
        let free_height =
            height - header - minimum_height - GAP * rows.len().saturating_sub(1) as f32;
        let extra_plot_height = (free_height / chart_rows.max(1) as f32).clamp(0., 64.);
        let mut contents = Vec::with_capacity(rows.len());
        let mut card_index = 0;
        for indices in rows {
            let row_cards = &cards[indices];
            let size: f32 = row_cards.iter().map(|card| card.width().min(width)).sum();
            let charts = row_cards
                .iter()
                .filter(|card| card.display == MonitorDisplay::Chart)
                .count();
            let extra = (width - size - GAP * row_cards.len().saturating_sub(1) as f32).max(0.)
                / charts.max(1) as f32;
            contents.push(split().items_start().flex_none().gap(r(GAP)).children(
                row_cards.iter().map(|card| {
                    let id = (id, card_index);
                    card_index += 1;
                    match card.display {
                        MonitorDisplay::Chart => plots::chart_card(
                            id,
                            theme,
                            card,
                            card.width().min(width) + extra,
                            extra_plot_height,
                        ),
                        display => compact_card(id.1, theme, &card.readings[0], display)
                            .w(r(card.width().min(width))),
                    }
                }),
            ));
        }
        col()
            .flex_1()
            .gap(r(GAP))
            .when(options.show_top_bar, |it| {
                it.child(self.monitor_top_bar(theme, width, header_height, wide))
            })
            .child(
                col()
                    .id(("monitor-readings", id as usize))
                    .flex_1()
                    .overflow_y_scroll()
                    .gap(r(GAP))
                    .when(contents.is_empty(), |it| {
                        it.child(theme.quiet(if options.readings.is_empty() {
                            "Choose readings in Edit layout → Setup."
                        } else {
                            "No visible drives."
                        }))
                    })
                    .children(contents),
            )
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
        let (value, unit, samples, detail, maximum) = match source {
            MonitorSource::Sensor(key) => {
                let sensor = self.sensor(key);
                let ram = matches!(key.as_str(), "memory" | "ram-used");
                let detail = (ram && self.metrics.memory_total > 0).then(|| {
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
                    detail,
                    maximum,
                )
            }
            MonitorSource::Download => (
                self.metrics.network.map(|rate| rate.download),
                "B/s",
                Some(&self.history.download),
                None,
                None,
            ),
            MonitorSource::Upload => (
                self.metrics.network.map(|rate| rate.upload),
                "B/s",
                Some(&self.history.upload),
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
                (None, "%", None, Some("Drive unavailable".into()), None)
            }
            MonitorSource::VisibleDrives => return None,
        };
        Some(Reading {
            name,
            value,
            unit,
            samples,
            detail,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_respects_order_and_leaves_room_for_compact_cards() {
        let widths = [128., 136., 176., 232., 128.];
        assert_eq!(card_rows(456., widths), [0..3, 3..5]);
        assert_eq!(card_rows(405., widths), [0..2, 2..3, 3..5]);
        assert_eq!(card_rows(205., widths), [0..1, 1..2, 2..3, 3..4, 4..5]);
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

fn card_frame(index: usize, theme: Theme, accent: Color) -> gpui::Stateful<Div> {
    col()
        .id(("monitor-reading", index))
        .flex_none()
        .overflow_hidden()
        .gap(r(4.))
        .p(r(7.))
        .rounded(r(6.))
        .border_1()
        .border_color(theme.color(accent).opacity(0.24))
        .bg(theme.color(BACK).opacity(0.48))
}

impl Reading<'_> {
    fn value_text(&self) -> String {
        self.value
            .map_or_else(|| "—".into(), |n| format_value(n, self.unit))
    }

    fn tooltip(&self, (low, high): (f32, f32)) -> SharedString {
        let mut label = format!("{} · {}", self.name, self.value_text());
        if let Some(detail) = &self.detail {
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

fn compact_card(
    index: usize,
    theme: Theme,
    reading: &Reading<'_>,
    display: MonitorDisplay,
) -> gpui::Stateful<Div> {
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
        .text_size(r(11.))
        .line_height(r(15.))
        .truncate();
    let value = mono(number.to_owned())
        .text_size(r(13.))
        .line_height(r(17.))
        .text_color(theme.color(accent))
        .truncate();
    let card = card_frame(index, theme, accent)
        .tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip.clone())).into());
    let card = if display == MonitorDisplay::Gauge {
        card.child(
            row()
                .gap(r(5.))
                .child(gauge(theme, normalized, accent, 43.))
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
        card.child(
            row()
                .justify_between()
                .gap(r(3.))
                .child(name.flex_1())
                .child(value.flex_none()),
        )
        .child(monitoring_bar(
            theme,
            normalized.unwrap_or_default(),
            accent,
        ))
    };
    card.children(
        reading
            .detail
            .as_ref()
            .map(|detail| reading_detail(theme, detail)),
    )
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
