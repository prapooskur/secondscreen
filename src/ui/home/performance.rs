use gpui::{AnyElement, Context, IntoElement, Styled, prelude::*};

use super::{
    super::{
        Dashboard,
        monitor::{graph, uptime},
        theme::*,
    },
    options::{SensorMode, SensorOptions},
};
use crate::model::Sensor;

impl Dashboard {
    fn sensor(&self, id: &str) -> Option<&Sensor> {
        if id == "gpu" {
            return self
                .metrics
                .sensors
                .iter()
                .filter(|s| s.id.starts_with("gpu:") && s.id.ends_with(":load"))
                .max_by_key(|s| s.value.is_some());
        }
        self.metrics.sensors.iter().find(|sensor| sensor.id == id)
    }

    pub(super) fn sensor_button(&self, id: u32, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let open = self
            .home
            .sensor_editor
            .is_some_and(|(active, _)| active == id);
        let view = cx.entity();
        theme.bind(
            theme
                .button(("sensors", id as usize), open.then_some(TEAL))
                .h(r(26.))
                .px(r(8.))
                .child(if open { "Done" } else { "Sensors" }),
            move |_, cx| {
                view.update(cx, |this, cx| {
                    this.home.sensor_editor = if open { None } else { Some((id, 0)) };
                    cx.notify();
                })
            },
        )
    }

    pub(super) fn performance_widget(
        &self,
        id: u32,
        height: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let options = self
            .home
            .layout()
            .sensors
            .get(&id)
            .cloned()
            .unwrap_or_default();
        if let Some((active, slot)) = self.home.sensor_editor
            && active == id
        {
            return self
                .sensor_editor(id, slot, &options, cx)
                .into_any_element();
        }
        let keys: &[String] = match options.mode {
            SensorMode::Chart => std::slice::from_ref(&options.chart),
            SensorMode::Pair => &options.pair,
            SensorMode::List => &options.list,
        };
        let list = options.mode == SensorMode::List;
        let mut readings = if list {
            col()
                .id(("sensor-list", id as usize))
                .overflow_y_scroll()
                .gap(r(7.))
        } else {
            split()
                .id(("sensor-readings", id as usize))
                .flex_1()
                .gap(r(12.))
        };
        for (index, key) in keys.iter().enumerate() {
            let sensor = self.sensor(key);
            let name = sensor.map(|s| s.name.as_str()).unwrap_or(if key == "gpu" {
                "GPU load"
            } else {
                key
            });
            let value = sensor
                .and_then(|s| s.value)
                .map(|n| {
                    let unit = sensor.unwrap().unit;
                    match unit {
                        "V" | "A" => format!("{n:.2} {unit}"),
                        "GiB" | "W" | "°C" => format!("{n:.1} {unit}"),
                        "%" => format!("{n:.0}%"),
                        _ => format!("{n:.0} {unit}"),
                    }
                })
                .unwrap_or_else(|| "—".into());
            let color = if index == 1 { BLUE } else { TEAL };
            if list {
                readings = readings.child(
                    row()
                        .justify_between()
                        .gap(r(8.))
                        .child(theme.label(name.to_owned()).truncate())
                        .child(mono(value).text_color(theme.color(color)).flex_none()),
                );
            } else {
                let mut card = col()
                    .flex_1()
                    .h_full()
                    .gap(r(3.))
                    .child(theme.quiet(name.to_owned()).truncate())
                    .child(
                        mono(value)
                            .text_size(r(if height > 85. { 26. } else { 17. }))
                            .text_color(theme.color(color))
                            .truncate(),
                    );
                if height >= 65.
                    && let Some(sensor) = sensor
                    && let Some(samples) = self.history.sensors.get(&sensor.id)
                {
                    let (low, high) = chart_range(samples.iter().copied(), sensor.unit);
                    card = card.child(graph(
                        theme,
                        samples.iter().map(|n| n - low).collect(),
                        None,
                        high - low,
                        color,
                    ));
                }
                readings = readings.child(card);
            }
        }
        col()
            .flex_1()
            .gap(r(4.))
            .child(readings)
            .when(keys.is_empty(), |it| {
                it.child(theme.quiet("Choose sensors to display."))
            })
            .when(!list && height >= 145., |it| {
                it.child(
                    row()
                        .justify_between()
                        .gap(r(6.))
                        .child(theme.quiet(self.metrics.cpu_name.clone()).truncate())
                        .children(self.metrics.uptime.map(|secs| {
                            theme
                                .quiet(format!("{} · {}", self.metrics.host, uptime(secs)))
                                .truncate()
                        })),
                )
            })
            .into_any_element()
    }

    fn sensor_editor(
        &self,
        id: u32,
        slot: usize,
        options: &SensorOptions,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let modes = [
            (SensorMode::Chart, "Chart"),
            (SensorMode::Pair, "2 sensors"),
            (SensorMode::List, "List"),
        ];
        let mut controls = row().flex_none().gap(r(4.));
        for (mode, title) in modes {
            let view = cx.entity();
            controls = controls.child(
                theme.bind(
                    theme
                        .button((title, id as usize), (mode == options.mode).then_some(TEAL))
                        .h(r(23.))
                        .px(r(6.))
                        .child(title),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.home.layout_mut().sensors.entry(id).or_default().mode = mode;
                            this.home.sensor_editor = Some((id, 0));
                            this.save_home(cx);
                        })
                    },
                ),
            );
        }
        let mut slots = row().gap(r(4.));
        if options.mode == SensorMode::Pair {
            for (i, key) in options.pair.iter().enumerate() {
                let view = cx.entity();
                let name = self.sensor(key).map(|s| s.name.as_str()).unwrap_or(key);
                slots = slots.child(
                    theme.bind(
                        theme
                            .button(
                                gpui::SharedString::from(format!("slot-{id}-{i}")),
                                (slot == i).then_some(BLUE),
                            )
                            .flex_1()
                            .min_w_0()
                            .h(r(22.))
                            .px(r(4.))
                            .child(theme.quiet(format!("{} · {name}", i + 1)).truncate()),
                        move |_, cx| {
                            view.update(cx, |this, cx| {
                                this.home.sensor_editor = Some((id, i));
                                cx.notify();
                            })
                        },
                    ),
                );
            }
        }
        let mut choices = col()
            .id(("sensor-choices", id as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(2.));
        let mut available: Vec<_> = self
            .metrics
            .sensors
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        // Keep disconnected selections reachable so a list can be edited after hardware changes.
        if options.mode == SensorMode::List {
            for key in &options.list {
                if self.sensor(key).is_none() {
                    available.push((key.clone(), format!("{key} · unavailable")));
                }
            }
        }
        for (index, (key, name)) in available.into_iter().enumerate() {
            let matches =
                |saved: &str| saved == key || self.sensor(saved).is_some_and(|s| s.id == key);
            let saved_key = options.list.iter().find(|k| matches(k)).cloned();
            let selected = match options.mode {
                SensorMode::Chart => matches(&options.chart),
                SensorMode::Pair => matches(&options.pair[slot]),
                SensorMode::List => saved_key.is_some(),
            };
            let view = cx.entity();
            choices = choices.child(
                theme.bind(
                    theme
                        .button(
                            gpui::SharedString::from(format!("sensor-choice-{id}-{index}")),
                            selected.then_some(TEAL),
                        )
                        .h(r(25.))
                        .px(r(6.))
                        .justify_start()
                        .child(theme.label(name).truncate()),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            let options = this.home.layout_mut().sensors.entry(id).or_default();
                            match options.mode {
                                SensorMode::Chart => options.chart = key.clone(),
                                SensorMode::Pair => options.pair[slot] = key.clone(),
                                SensorMode::List => {
                                    if let Some(saved) = &saved_key {
                                        options.list.retain(|s| s != saved)
                                    } else {
                                        options.list.push(key.clone())
                                    }
                                }
                            }
                            this.save_home(cx);
                        })
                    },
                ),
            );
        }
        col()
            .flex_1()
            .gap(r(3.))
            .child(controls)
            .when(options.mode == SensorMode::Pair, |it| it.child(slots))
            .child(choices)
    }
}

fn chart_range(values: impl Iterator<Item = f32>, unit: &str) -> (f32, f32) {
    if unit == "%" {
        return (0., 100.);
    }
    let (low, high) = values.fold((0f32, 0f32), |(lo, hi), v| (lo.min(v), hi.max(v)));
    (low, high.max(low + 1.))
}
