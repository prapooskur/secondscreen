use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, Styled, prelude::*};

use super::super::{
    Dashboard, HISTORY,
    icons::{Icon, icon},
    monitor::{gauge, gib, monitor_graph, monitoring_bar, rate, storage_list, uptime},
    percent,
    theme::*,
};

const GAP: f32 = 8.;
// The sensor charts occupy 0 and 1 in the widget's chart cache.
const MEMORY_CHART: usize = 2;
const NETWORK_CHART: usize = 3;

impl Dashboard {
    pub(super) fn system_monitor_widget(
        &self,
        id: u32,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let layout = self.home.layout();
        let options = layout.sensors.get(&id).cloned().unwrap_or_default();
        if let Some((active, slot)) = self.home.sensor_editor
            && active == id
        {
            return self
                .sensor_editor(id, slot, &options, cx)
                .into_any_element();
        }

        let wide = width >= 408.;
        let header_height = if wide { 32. } else { 44. };
        let body_height = height - header_height - GAP;
        let performance = section(theme, Icon::Activity, "PERFORMANCE", TEAL)
            .child(self.performance_readings(id, theme, &options));
        let memory = self.memory_section(id, theme);
        let network = self.network_section(id, theme);
        let storage = section(theme, Icon::Disk, "STORAGE", BLUE).child(storage_list(
            theme,
            ("monitor-drives", id as usize),
            self.visible_disks(),
        ));

        let grid = col()
            .id(("system-monitor", id as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(GAP));
        let grid = if wide {
            let row_height = ((body_height - GAP) / 2.).max(123.);
            grid.child(
                split()
                    .flex_none()
                    .h(r(row_height))
                    .gap(r(GAP))
                    .child(performance)
                    .child(memory),
            )
            .child(
                split()
                    .flex_none()
                    .h(r(row_height))
                    .gap(r(GAP))
                    .child(network)
                    .child(storage),
            )
        } else {
            let card_height = ((body_height - 3. * GAP) / 4.).max(104.);
            grid.child(performance.h(r(card_height.max(123.))).flex_none())
                .child(memory.h(r(card_height)).flex_none())
                .child(network.h(r(card_height.max(119.))).flex_none())
                .child(storage.h(r(card_height.max(112.))).flex_none())
        };
        let host = self
            .metrics
            .uptime
            .map(|seconds| format!("{} · {}", self.metrics.host, uptime(seconds)));
        let host_label = |text| {
            theme
                .quiet(text)
                .text_size(r(10.))
                .line_height(r(13.))
                .truncate()
        };
        col()
            .flex_1()
            .gap(r(GAP))
            .child(
                row()
                    .h(r(header_height))
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
                                it.child(
                                    theme
                                        .quiet(self.metrics.cpu_name.clone())
                                        .text_size(r(10.))
                                        .line_height(r(13.))
                                        .truncate(),
                                )
                            })
                            .children(host.as_ref().filter(|_| !wide).cloned().map(host_label)),
                    )
                    .children(
                        host.filter(|_| wide)
                            .map(|text| host_label(text).max_w(r(width * 0.4))),
                    ),
            )
            .child(grid)
            .into_any_element()
    }

    fn memory_section(&self, id: u32, theme: Theme) -> Div {
        let memory = percent(self.metrics.memory_used, self.metrics.memory_total);
        let capacity = memory.map_or_else(
            || "RAM usage unavailable".to_owned(),
            |_| {
                format!(
                    "{:.1} of {:.1} GiB used",
                    gib(self.metrics.memory_used),
                    gib(self.metrics.memory_total)
                )
            },
        );
        section(theme, Icon::Memory, "MEMORY", VIOLET).child(
            split()
                .flex_1()
                .min_h_0()
                .gap(r(7.))
                .child(gauge(theme, memory, VIOLET, 62.).self_center())
                .child(
                    col()
                        .flex_1()
                        .gap(r(4.))
                        .child(monitor_graph(
                            (id, MEMORY_CHART),
                            theme,
                            self.history.memory.clone(),
                            None,
                            100.,
                            VIOLET,
                        ))
                        .children(memory.map(|load| monitoring_bar(theme, load, VIOLET)))
                        .child(theme.quiet(capacity).text_size(r(10.)).truncate()),
                ),
        )
    }

    fn network_section(&self, id: u32, theme: Theme) -> Div {
        let network = self.metrics.network;
        let peak = self
            .history
            .download
            .iter()
            .chain(&self.history.upload)
            .copied()
            .fold(1024., f32::max);
        let duration = if network.is_some() {
            format!("Last {HISTORY} seconds")
        } else {
            "Waiting for network data".to_owned()
        };
        section(theme, Icon::Network, "NETWORK", AMBER)
            .child(
                split()
                    .flex_none()
                    .gap(r(7.))
                    .child(network_rate(
                        theme,
                        "Download",
                        network.map(|n| n.download),
                        AMBER,
                    ))
                    .child(network_rate(
                        theme,
                        "Upload",
                        network.map(|n| n.upload),
                        BLUE,
                    )),
            )
            .child(monitor_graph(
                (id, NETWORK_CHART),
                theme,
                self.history.download.clone(),
                Some(self.history.upload.clone()),
                peak * 1.08,
                AMBER,
            ))
            .child(
                theme
                    .quiet(duration)
                    .text_size(r(10.))
                    .line_height(r(13.))
                    .truncate(),
            )
    }
}

fn section(theme: Theme, symbol: Icon, name: &'static str, accent: Color) -> Div {
    col()
        .flex_1()
        .overflow_hidden()
        .gap(r(5.))
        .p(r(7.))
        .rounded(r(6.))
        .border_1()
        .border_color(theme.color(accent).opacity(0.24))
        .bg(theme.color(BACK).opacity(0.48))
        .child(
            row()
                .flex_none()
                .gap(r(6.))
                .child(icon(symbol, theme.color(accent), 12.))
                .child(
                    mono(name)
                        .text_size(r(11.))
                        .line_height(r(15.))
                        .text_color(theme.color(accent)),
                ),
        )
}

fn network_rate(theme: Theme, name: &'static str, amount: Option<f32>, accent: Color) -> Div {
    let reading = amount.map_or_else(
        || "—".into(),
        |bytes| {
            let (number, unit) = rate(bytes);
            format!("{number} {unit}")
        },
    );
    col()
        .flex_1()
        .child(
            theme
                .quiet(name)
                .text_size(r(10.))
                .line_height(r(13.))
                .truncate(),
        )
        .child(
            mono(reading)
                .text_size(r(14.))
                .line_height(r(18.))
                .text_color(theme.color(accent))
                .truncate(),
        )
}
