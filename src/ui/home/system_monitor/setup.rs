use gpui::{Context, Div, Entity, SharedString, Stateful, Styled, Window, prelude::*};

use crate::ui::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    monitor::discard_charts,
    text_input::TextInput,
    theme::*,
};

use super::super::{
    Kind,
    options::{
        ChartLayout, MAX_PLOTS, MonitorDisplay, MonitorOptions, MonitorReading, MonitorSource,
    },
};

pub(in crate::ui::home) struct MonitorSetup {
    pub id: u32,
    chart: Option<usize>,
    picker: Option<(Pick, Entity<TextInput>)>,
}

impl MonitorSetup {
    pub(in crate::ui::home) fn back(&mut self) -> bool {
        self.picker.take().is_some() || self.chart.take().is_some()
    }
}

#[derive(Clone, Copy)]
enum Pick {
    Card,
    Primary(usize),
    Comparison(usize),
}

#[derive(Clone, Copy)]
enum Navigate {
    Back,
    Chart(usize),
    Pick(Pick),
}

#[derive(Clone)]
enum Change {
    TopBar,
    Select(Pick, MonitorReading),
    Display(usize, MonitorDisplay),
    Layout(usize, ChartLayout),
    RemovePlot(usize, usize),
    Move(usize, isize),
    Remove(usize),
}

impl Dashboard {
    pub(in crate::ui::home) fn monitor_setup_button(
        &self,
        id: u32,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let open = self
            .home
            .monitor_setup
            .as_ref()
            .is_some_and(|setup| setup.id == id);
        let view = cx.entity();
        theme.bind(
            theme
                .button(("monitor-setup", id as usize), open.then_some(TEAL))
                .h(r(26.))
                .px(r(8.))
                .child(if open { "Done" } else { "Setup" }),
            move |window, cx| {
                view.update(cx, |this, cx| {
                    this.home.monitor_setup = (!open).then_some(MonitorSetup {
                        id,
                        chart: None,
                        picker: None,
                    });
                    this.focus_dashboard(window, cx);
                    cx.notify();
                })
            },
        )
    }

    pub(super) fn monitor_setup_menu(
        &self,
        setup: &MonitorSetup,
        options: &MonitorOptions,
        width: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let id = setup.id;
        let (controls, body) = if let Some((pick, search)) = &setup.picker {
            (
                row().child(navigation_button(theme, id, "Back", Navigate::Back, cx)),
                col()
                    .flex_1()
                    .gap(r(5.))
                    .child(search.clone())
                    .child(self.monitor_sensor_catalog(
                        id,
                        *pick,
                        &search.read(cx).value,
                        options,
                        cx,
                    ))
                    .into_any_element(),
            )
        } else if let Some(index) = setup.chart
            && let Some(reading) = options.readings.get(index)
        {
            (
                row()
                    .justify_between()
                    .child(navigation_button(theme, id, "Back", Navigate::Back, cx))
                    .when(reading.chart.additional.len() + 1 < MAX_PLOTS, |it| {
                        it.child(navigation_button(
                            theme,
                            id,
                            "Add metric",
                            Navigate::Pick(Pick::Comparison(index)),
                            cx,
                        ))
                    }),
                self.monitor_chart_setup(id, index, reading, cx)
                    .into_any_element(),
            )
        } else {
            let top_bar = theme
                .button("monitor-top-bar", options.show_top_bar.then_some(TEAL))
                .h(r(26.))
                .px(r(8.))
                .child(if options.show_top_bar {
                    "Top bar on"
                } else {
                    "Top bar off"
                });
            (
                row()
                    .justify_between()
                    .child(navigation_button(
                        theme,
                        id,
                        "Add sensor",
                        Navigate::Pick(Pick::Card),
                        cx,
                    ))
                    .child(edit_button(theme, top_bar, id, Change::TopBar, cx)),
                self.monitor_selected_sensors(id, options, width >= 380., cx)
                    .into_any_element(),
            )
        };
        col()
            .id(("monitor-setup-menu", id as usize))
            .flex_1()
            .gap(r(6.))
            .child(controls.flex_none().gap(r(5.)))
            .child(body)
    }

    fn monitor_chart_setup(
        &self,
        id: u32,
        index: usize,
        reading: &MonitorReading,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let layouts = [
            (ChartLayout::Separate, "Separate plots"),
            (ChartLayout::Overlay, "Shared graph"),
        ];
        col()
            .flex_1()
            .gap(r(6.))
            .child(
                row()
                    .flex_none()
                    .gap(r(4.))
                    .children(layouts.map(|(layout, name)| {
                        let button = theme
                            .button(name, (layout == reading.chart.layout).then_some(TEAL))
                            .h(r(23.))
                            .px(r(5.))
                            .text_size(r(11.))
                            .child(name);
                        edit_button(theme, button, id, Change::Layout(index, layout), cx)
                    })),
            )
            .child(
                col()
                    .id("monitor-plot-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .gap(r(4.))
                    .child(
                        row()
                            .flex_none()
                            .gap(r(5.))
                            .child(
                                theme
                                    .label(self.monitor_source_name(&reading.source).to_owned())
                                    .flex_1()
                                    .truncate(),
                            )
                            .child(navigation_button(
                                theme,
                                id,
                                "Change metric",
                                Navigate::Pick(Pick::Primary(index)),
                                cx,
                            )),
                    )
                    .children(
                        reading
                            .chart
                            .additional
                            .iter()
                            .enumerate()
                            .map(|(slot, source)| {
                                let remove = theme
                                    .button(("remove-plot", slot), None)
                                    .size(r(24.))
                                    .px_0()
                                    .child(icon(Icon::Close, theme.color(RED), 12.))
                                    .tooltip(|_, cx| {
                                        cx.new(|_| Tooltip("Remove metric".into())).into()
                                    });
                                row()
                                    .flex_none()
                                    .gap(r(5.))
                                    .child(
                                        theme
                                            .label(self.monitor_source_name(source).to_owned())
                                            .flex_1()
                                            .truncate(),
                                    )
                                    .child(edit_button(
                                        theme,
                                        remove,
                                        id,
                                        Change::RemovePlot(index, slot),
                                        cx,
                                    ))
                            }),
                    ),
            )
    }

    fn monitor_selected_sensors(
        &self,
        id: u32,
        options: &MonitorOptions,
        wide: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        col()
            .id(("monitor-sensors", id as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(4.))
            .when(options.readings.is_empty(), |list| {
                list.child(theme.quiet("No sensors selected."))
            })
            .children(options.readings.iter().enumerate().map(|(index, reading)| {
                let mut title = self.monitor_source_name(&reading.source).to_owned();
                if reading.display == MonitorDisplay::Chart {
                    for source in &reading.chart.additional {
                        title.push_str(" · ");
                        title.push_str(self.monitor_source_name(source));
                    }
                }
                let name = SharedString::from(title);
                let label = theme
                    .label(name.clone())
                    .id("reading-name")
                    .flex_1()
                    .text_size(r(12.))
                    .truncate()
                    .tooltip(move |_, cx| cx.new(|_| Tooltip(name.clone())).into());
                let displays = row()
                    .flex_none()
                    .gap(r(3.))
                    .children(MonitorDisplay::ALL.map(|display| {
                        let button = theme
                            .button(display.name(), (display == reading.display).then_some(TEAL))
                            .h(r(23.))
                            .px(r(5.))
                            .text_size(r(11.))
                            .child(display.name());
                        edit_button(theme, button, id, Change::Display(index, display), cx)
                    }));
                let moves = row().flex_none().gap(r(3.)).children(
                    [
                        (
                            "Move earlier",
                            Icon::Up,
                            Change::Move(index, -1),
                            index > 0,
                            TEXT,
                        ),
                        (
                            "Move later",
                            Icon::Down,
                            Change::Move(index, 1),
                            index + 1 < options.readings.len(),
                            TEXT,
                        ),
                        (
                            "Remove sensor",
                            Icon::Close,
                            Change::Remove(index),
                            true,
                            RED,
                        ),
                    ]
                    .map(|(label, symbol, change, enabled, accent)| {
                        let button = theme
                            .button(label, None)
                            .size(r(23.))
                            .px_0()
                            .child(icon(symbol, theme.color(accent), 12.))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip(label.into())).into());
                        if enabled {
                            edit_button(theme, button, id, change, cx)
                        } else {
                            disabled(button)
                        }
                    }),
                );
                let item = col()
                    .id(("monitor-selected", index))
                    .flex_none()
                    .gap(r(4.))
                    .p(r(4.))
                    .rounded(r(6.))
                    .bg(theme.color(HEADER));
                let item = if wide {
                    item.child(row().gap(r(5.)).child(label).child(displays).child(moves))
                } else {
                    item.child(row().gap(r(5.)).child(label).child(moves))
                        .child(displays)
                };
                let (label, page) = if reading.display == MonitorDisplay::Chart
                    && reading.source != MonitorSource::VisibleDrives
                {
                    ("Edit plots", Navigate::Chart(index))
                } else {
                    ("Change metric", Navigate::Pick(Pick::Primary(index)))
                };
                item.child(row().child(navigation_button(theme, id, label, page, cx)))
            }))
    }

    fn monitor_sensor_catalog(
        &self,
        id: u32,
        pick: Pick,
        search: &str,
        options: &MonitorOptions,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        use MonitorDisplay::{Bar, Chart, Gauge};
        use MonitorSource::{Download, Drive, Sensor, Upload, VisibleDrives};

        let theme = Theme::current(cx);
        let query = search.trim().to_lowercase();
        let common = [
            (Sensor("gpu".into()), "GPU load (automatic)", Gauge),
            (Download, "Download", Chart),
            (Upload, "Upload", Chart),
            (VisibleDrives, "All visible drives", Bar),
        ];
        let sensors = self.metrics.sensors.iter().map(|sensor| {
            (
                Sensor(sensor.id.clone()),
                sensor.name.as_str(),
                if sensor.unit == "%" { Gauge } else { Chart },
            )
        });
        let drives = self
            .visible_disks()
            .into_iter()
            .map(|disk| (Drive(disk.id.clone()), disk.label.as_str(), Bar));
        let mut list = col()
            .id(("monitor-sensor-catalog", id as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(3.));
        let mut found = false;
        for (index, (source, name, display)) in
            common.into_iter().chain(drives).chain(sensors).enumerate()
        {
            if (!query.is_empty() && !name.to_lowercase().contains(&query))
                || !can_pick(options, pick, &source)
            {
                continue;
            }
            found = true;
            let name = SharedString::from(name.to_owned());
            let button = theme
                .button(("monitor-available", index), None)
                .h(r(26.))
                .px(r(6.))
                .justify_between()
                .child(
                    theme
                        .label(name.clone())
                        .flex_1()
                        .text_size(r(12.))
                        .truncate(),
                )
                .child(icon(Icon::Plus, theme.color(TEAL), 13.))
                .tooltip(move |_, cx| cx.new(|_| Tooltip(name.clone())).into());
            list = list.child(edit_button(
                theme,
                button,
                id,
                Change::Select(pick, MonitorReading::new(source, display)),
                cx,
            ));
        }
        list.when(!found, |it| {
            it.child(theme.quiet("No matching sensors to add."))
        })
    }

    fn edit_monitor(
        &mut self,
        id: u32,
        change: Change,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .home
            .monitor_setup
            .as_ref()
            .is_some_and(|setup| setup.id == id)
        {
            return;
        }
        let layout = self.home.layout_mut();
        if !layout
            .widgets
            .iter()
            .any(|widget| widget.id == id && widget.kind == Kind::SystemMonitor)
        {
            return;
        }
        let mut options = layout.monitors.get(&id).cloned().unwrap_or_default();
        let close_picker = matches!(
            change,
            Change::Select(Pick::Primary(_) | Pick::Comparison(_), _)
        );
        match change {
            Change::TopBar => options.show_top_bar = !options.show_top_bar,
            Change::Select(pick, reading) => {
                if !can_pick(&options, pick, &reading.source) {
                    return;
                }
                match pick {
                    Pick::Card => options.readings.push(reading),
                    Pick::Primary(index) => options.readings[index].source = reading.source,
                    Pick::Comparison(index) => options.readings[index]
                        .chart
                        .additional
                        .push(reading.source),
                }
            }
            Change::Layout(index, chart_layout) => {
                let Some(reading) = options.readings.get_mut(index) else {
                    return;
                };
                if reading.chart.layout == chart_layout {
                    return;
                }
                reading.chart.layout = chart_layout;
            }
            Change::RemovePlot(index, slot) => {
                let Some(reading) = options.readings.get_mut(index) else {
                    return;
                };
                if slot >= reading.chart.additional.len() {
                    return;
                }
                reading.chart.additional.remove(slot);
            }
            Change::Display(index, display) => {
                let Some(reading) = options.readings.get_mut(index) else {
                    return;
                };
                if reading.display == display {
                    return;
                }
                reading.display = display;
            }
            Change::Move(index, step) => {
                let Some(target) = index
                    .checked_add_signed(step)
                    .filter(|&target| target < options.readings.len())
                else {
                    return;
                };
                if index >= options.readings.len() {
                    return;
                }
                options.readings.swap(index, target);
            }
            Change::Remove(index) => {
                if index >= options.readings.len() {
                    return;
                }
                options.readings.remove(index);
            }
        }
        let previous = layout.monitors.insert(id, options);
        if self.save_home(cx) {
            discard_charts(Some(id), window, cx);
            if close_picker && let Some(setup) = &mut self.home.monitor_setup {
                setup.picker = None;
                self.focus_dashboard(window, cx);
            }
        } else if let Some(previous) = previous {
            self.home.layout_mut().monitors.insert(id, previous);
        } else {
            self.home.layout_mut().monitors.remove(&id);
        }
    }
}

fn edit_button(
    theme: Theme,
    button: Stateful<Div>,
    id: u32,
    change: Change,
    cx: &Context<Dashboard>,
) -> Stateful<Div> {
    let view = cx.entity();
    theme.bind(button, move |window, cx| {
        view.update(cx, |this, cx| {
            this.edit_monitor(id, change.clone(), window, cx)
        });
    })
}

fn can_pick(options: &MonitorOptions, pick: Pick, source: &MonitorSource) -> bool {
    match pick {
        Pick::Card => true,
        Pick::Primary(index) => options.readings.get(index).is_some_and(|reading| {
            reading.source != *source
                && !reading.chart.additional.contains(source)
                && (reading.chart.additional.is_empty() || *source != MonitorSource::VisibleDrives)
        }),
        Pick::Comparison(index) => options.readings.get(index).is_some_and(|reading| {
            reading.display == MonitorDisplay::Chart
                && reading.source != MonitorSource::VisibleDrives
                && *source != MonitorSource::VisibleDrives
                && reading.source != *source
                && !reading.chart.additional.contains(source)
                && reading.chart.additional.len() + 1 < MAX_PLOTS
        }),
    }
}

fn navigation_button(
    theme: Theme,
    id: u32,
    label: &'static str,
    navigate: Navigate,
    cx: &Context<Dashboard>,
) -> Stateful<Div> {
    let view = cx.entity();
    theme.bind(
        theme
            .button(
                label,
                matches!(navigate, Navigate::Pick(Pick::Card | Pick::Comparison(_)))
                    .then_some(TEAL),
            )
            .h(r(24.))
            .px(r(7.))
            .text_size(r(11.))
            .child(label),
        move |window, cx| {
            view.update(cx, |this, cx| {
                if let Some(setup) = &mut this.home.monitor_setup
                    && setup.id == id
                {
                    match navigate {
                        Navigate::Back => {
                            setup.back();
                        }
                        Navigate::Chart(index) => setup.chart = Some(index),
                        Navigate::Pick(target) => {
                            let search =
                                cx.new(|cx| TextInput::with_placeholder("Search sensors", cx));
                            cx.observe(&search, |_, _, cx| cx.notify()).detach();
                            setup.picker = Some((target, search));
                        }
                    }
                    this.focus_dashboard(window, cx);
                    cx.notify();
                }
            })
        },
    )
}
