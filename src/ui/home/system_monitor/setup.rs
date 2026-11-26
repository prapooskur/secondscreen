use gpui::{
    AnyElement, Context, Div, Entity, SharedString, Stateful, Styled, Window, div, prelude::*,
};

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
    page: Page,
}

enum Page {
    Cards,
    AddCard,
    Edit(usize),
    Pick(Pick, Entity<TextInput>),
}

impl MonitorSetup {
    pub(in crate::ui::home) fn back(&mut self) -> bool {
        self.page = match self.page {
            Page::Cards => return false,
            Page::AddCard | Page::Edit(_) => Page::Cards,
            Page::Pick(Pick::New(_), _) => Page::AddCard,
            Page::Pick(Pick::Primary(index) | Pick::Comparison(index), _) => Page::Edit(index),
        };
        true
    }
}

#[derive(Clone, Copy)]
enum Pick {
    New(MonitorDisplay),
    Primary(usize),
    Comparison(usize),
}

#[derive(Clone, Copy)]
enum Navigate {
    Back,
    AddCard,
    Edit(usize),
    Pick(Pick),
}

#[derive(Clone)]
enum Change {
    TopBar,
    Select(Pick, MonitorSource),
    Display(usize, MonitorDisplay),
    Layout(usize, ChartLayout),
    RemovePlot(usize, usize),
    Move(usize, isize),
    Remove(usize),
}

impl Dashboard {
    fn set_monitor_setup(
        &mut self,
        id: u32,
        page: Option<Page>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.home.fps_setup = None;
        self.home.monitor_setup = page.map(|page| MonitorSetup { id, page });
        self.focus_dashboard(window, cx);
        cx.notify();
    }

    pub(super) fn monitor_add_cards_button(&self, id: u32, cx: &Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let view = cx.entity();
        theme.bind(
            theme
                .button(("monitor-add-cards", id as usize), Some(TEAL))
                .child("Add cards"),
            move |window, cx| {
                view.update(cx, |this, cx| {
                    this.set_monitor_setup(id, Some(Page::AddCard), window, cx);
                })
            },
        )
    }

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
                    this.set_monitor_setup(id, (!open).then_some(Page::Cards), window, cx);
                })
            },
        )
    }

    pub(super) fn monitor_setup_menu(
        &self,
        setup: &MonitorSetup,
        options: &MonitorOptions,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let id = setup.id;
        let back = || navigation_button(theme, id, "Back", Navigate::Back, cx);
        let (controls, body) = match &setup.page {
            Page::AddCard => (
                row()
                    .child(back())
                    .child(theme.label("Display type").truncate()),
                self.monitor_card_types(id, theme, cx).into_any_element(),
            ),
            Page::Pick(pick, search) => {
                let display = match *pick {
                    Pick::New(display) => Some(display),
                    Pick::Primary(index) | Pick::Comparison(index) => {
                        options.readings.get(index).map(|card| card.display)
                    }
                };
                (
                    row().child(back()).children(display.map(|display| {
                        theme.label(format!("{} metric", display.name())).truncate()
                    })),
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
            }
            Page::Edit(index) if let Some(card) = options.readings.get(*index) => (
                row()
                    .justify_between()
                    .child(back())
                    .when(can_add_plot(card), |it| {
                        it.child(navigation_button(
                            theme,
                            id,
                            "Add metric",
                            Navigate::Pick(Pick::Comparison(*index)),
                            cx,
                        ))
                    }),
                self.monitor_edit_card(id, *index, card, cx)
                    .into_any_element(),
            ),
            Page::Cards | Page::Edit(_) => {
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
                            "Add card",
                            Navigate::AddCard,
                            cx,
                        ))
                        .child(edit_button(theme, top_bar, id, Change::TopBar, cx)),
                    self.monitor_selected_cards(id, options, cx)
                        .into_any_element(),
                )
            }
        };
        col()
            .id(("monitor-setup-menu", id as usize))
            .flex_1()
            .gap(r(6.))
            .child(
                row()
                    .flex_none()
                    .gap(r(5.))
                    .child(controls.flex_1().min_w_0().gap(r(5.)))
                    .when(!self.home.editing, |it| {
                        it.child(self.monitor_setup_button(id, cx))
                    }),
            )
            .child(body)
    }

    fn monitor_card_types(&self, id: u32, theme: Theme, cx: &Context<Self>) -> Div {
        row()
            .flex_none()
            .gap(r(5.))
            .children(MonitorDisplay::ALL.map(|display| {
                let button = theme
                    .button(display.name(), Some(display_accent(display)))
                    .flex_1()
                    .min_w_0()
                    .h(r(57.))
                    .px(r(3.))
                    .flex_col()
                    .gap(r(5.))
                    .child(display_icon(display, theme, 18.))
                    .child(display.name());
                bind_navigation(theme, button, id, Navigate::Pick(Pick::New(display)), cx)
            }))
    }

    fn monitor_edit_card(
        &self,
        id: u32,
        index: usize,
        card: &MonitorReading,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let multi_plot =
            card.display == MonitorDisplay::Chart && card.source != MonitorSource::VisibleDrives;
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
                    .children(MonitorDisplay::ALL.map(|display| {
                        let button = theme
                            .button(
                                display.name(),
                                (display == card.display).then_some(display_accent(display)),
                            )
                            .h(r(24.))
                            .px(r(7.))
                            .text_size(r(11.))
                            .child(display.name());
                        edit_button(theme, button, id, Change::Display(index, display), cx)
                    })),
            )
            .when(multi_plot, |it| {
                it.child(
                    row()
                        .flex_none()
                        .gap(r(4.))
                        .children(layouts.map(|(layout, name)| {
                            let button = theme
                                .button(name, (layout == card.chart.layout).then_some(TEAL))
                                .h(r(23.))
                                .px(r(5.))
                                .text_size(r(11.))
                                .child(name);
                            edit_button(theme, button, id, Change::Layout(index, layout), cx)
                        })),
                )
            })
            .child(
                col()
                    .id("monitor-card-metrics")
                    .flex_1()
                    .overflow_y_scroll()
                    .gap(r(4.))
                    .child(
                        row()
                            .flex_none()
                            .gap(r(5.))
                            .child(
                                theme
                                    .label(self.monitor_source_name(&card.source).to_owned())
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
                        card.chart
                            .additional
                            .iter()
                            .filter(|_| multi_plot)
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

    fn monitor_selected_cards(
        &self,
        id: u32,
        options: &MonitorOptions,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        col()
            .id(("monitor-cards", id as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(5.))
            .when(options.readings.is_empty(), |list| {
                list.child(theme.quiet("No cards yet."))
            })
            .children(options.readings.iter().enumerate().map(|(index, card)| {
                let metrics = std::iter::once(&card.source)
                    .chain(
                        card.chart
                            .additional
                            .iter()
                            .filter(|_| card.display == MonitorDisplay::Chart),
                    )
                    .map(|source| self.monitor_source_name(source))
                    .collect::<Vec<_>>()
                    .join(" · ");
                let name = SharedString::from(metrics);
                let accent = display_accent(card.display);
                let controls = [
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
                    ("Remove card", Icon::Close, Change::Remove(index), true, RED),
                ]
                .map(|(label, symbol, change, enabled, color)| {
                    let button = theme
                        .button(label, None)
                        .size(r(23.))
                        .px_0()
                        .child(icon(symbol, theme.color(color), 12.))
                        .tooltip(move |_, cx| cx.new(|_| Tooltip(label.into())).into());
                    if enabled {
                        edit_button(theme, button, id, change, cx)
                    } else {
                        disabled(button)
                    }
                });
                col()
                    .id(("monitor-card", index))
                    .flex_none()
                    .gap(r(4.))
                    .p(r(5.))
                    .rounded(r(6.))
                    .bg(theme.color(HEADER))
                    .child(
                        row()
                            .gap(r(4.))
                            .child(display_icon(card.display, theme, 15.))
                            .child(
                                theme
                                    .label(card.display.name())
                                    .flex_1()
                                    .text_color(theme.color(accent))
                                    .truncate(),
                            )
                            .child(navigation_button(
                                theme,
                                id,
                                "Edit",
                                Navigate::Edit(index),
                                cx,
                            ))
                            .children(controls),
                    )
                    .child(
                        theme
                            .quiet(name.clone())
                            .id("card-metrics")
                            .text_size(r(11.))
                            .line_height(r(15.))
                            .truncate()
                            .tooltip(move |_, cx| cx.new(|_| Tooltip(name.clone())).into()),
                    )
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
        use MonitorSource::{Download, Drive, Sensor, Upload, VisibleDrives};

        let theme = Theme::current(cx);
        let query = search.trim().to_lowercase();
        let common = [
            (Sensor("gpu".into()), "GPU load (automatic)"),
            (Download, "Download"),
            (Upload, "Upload"),
            (VisibleDrives, "All visible drives"),
        ];
        let sensors = self
            .metrics
            .sensors
            .iter()
            .map(|sensor| (Sensor(sensor.id.clone()), sensor.name.as_str()));
        let drives = self
            .visible_disks()
            .into_iter()
            .map(|disk| (Drive(disk.id.clone()), disk.label.as_str()));
        let mut list = col()
            .id(("monitor-sensor-catalog", id as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(3.));
        let mut found = false;
        for (index, (source, name)) in common.into_iter().chain(drives).chain(sensors).enumerate() {
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
                Change::Select(pick, source),
                cx,
            ));
        }
        list.when(!found, |it| it.child(theme.quiet("No matching metrics.")))
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
        let mut next_page = None;
        match change {
            Change::TopBar => options.show_top_bar = !options.show_top_bar,
            Change::Select(pick, source) => {
                if !can_pick(&options, pick, &source) {
                    return;
                }
                next_page = Some(match pick {
                    Pick::New(display) => {
                        let index = options.readings.len();
                        options.readings.push(MonitorReading::new(source, display));
                        if display == MonitorDisplay::Chart {
                            Page::Edit(index)
                        } else {
                            Page::Cards
                        }
                    }
                    Pick::Primary(index) => {
                        options.readings[index].source = source;
                        Page::Edit(index)
                    }
                    Pick::Comparison(index) => {
                        options.readings[index].chart.additional.push(source);
                        Page::Edit(index)
                    }
                });
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
            if let Some(page) = next_page
                && let Some(setup) = &mut self.home.monitor_setup
            {
                setup.page = page;
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
        Pick::New(_) => true,
        Pick::Primary(index) => options.readings.get(index).is_some_and(|reading| {
            reading.source != *source
                && !reading.chart.additional.contains(source)
                && (reading.chart.additional.is_empty() || *source != MonitorSource::VisibleDrives)
        }),
        Pick::Comparison(index) => options.readings.get(index).is_some_and(|reading| {
            can_add_plot(reading)
                && *source != MonitorSource::VisibleDrives
                && reading.source != *source
                && !reading.chart.additional.contains(source)
        }),
    }
}

fn can_add_plot(card: &MonitorReading) -> bool {
    card.display == MonitorDisplay::Chart
        && card.source != MonitorSource::VisibleDrives
        && card.chart.additional.len() + 1 < MAX_PLOTS
}

fn display_accent(display: MonitorDisplay) -> Color {
    match display {
        MonitorDisplay::Bar => BLUE,
        MonitorDisplay::Chart => TEAL,
        MonitorDisplay::Gauge => VIOLET,
    }
}

fn display_icon(display: MonitorDisplay, theme: Theme, size: f32) -> AnyElement {
    let accent = theme.color(display_accent(display));
    match display {
        MonitorDisplay::Bar => row()
            .size(r(size))
            .gap(r(2.))
            .flex_none()
            .children((0..3).map(|index| {
                div()
                    .flex_1()
                    .h(r(4.))
                    .rounded(r(1.))
                    .bg(accent.opacity(if index < 2 { 1. } else { 0.27 }))
            }))
            .into_any_element(),
        MonitorDisplay::Chart => icon(Icon::Activity, accent, size).into_any_element(),
        MonitorDisplay::Gauge => icon(Icon::Gauge, accent, size).into_any_element(),
    }
}

fn navigation_button(
    theme: Theme,
    id: u32,
    label: &'static str,
    navigate: Navigate,
    cx: &Context<Dashboard>,
) -> Stateful<Div> {
    let primary = matches!(
        navigate,
        Navigate::AddCard | Navigate::Pick(Pick::Comparison(_))
    );
    let button = theme
        .button(label, primary.then_some(TEAL))
        .h(r(24.))
        .px(r(7.))
        .text_size(r(11.))
        .child(label);
    bind_navigation(theme, button, id, navigate, cx)
}

fn bind_navigation(
    theme: Theme,
    button: Stateful<Div>,
    id: u32,
    navigate: Navigate,
    cx: &Context<Dashboard>,
) -> Stateful<Div> {
    let view = cx.entity();
    theme.bind(button, move |window, cx| {
        view.update(cx, |this, cx| {
            if let Some(setup) = &mut this.home.monitor_setup
                && setup.id == id
            {
                match navigate {
                    Navigate::Back => {
                        setup.back();
                    }
                    Navigate::AddCard => setup.page = Page::AddCard,
                    Navigate::Edit(index) => setup.page = Page::Edit(index),
                    Navigate::Pick(target) => {
                        let search = cx.new(|cx| TextInput::with_placeholder("Search sensors", cx));
                        cx.observe(&search, |_, _, cx| cx.notify()).detach();
                        setup.page = Page::Pick(target, search);
                    }
                }
                this.focus_dashboard(window, cx);
                cx.notify();
            }
        })
    })
}
