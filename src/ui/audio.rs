use gpui::{AnyElement, Context, ElementId, IntoElement, Styled, div, prelude::*};

use super::{
    Dashboard,
    icons::{Icon, icon},
    slider::slider,
    theme::*,
};
use crate::model::{AudioAction, AudioChannel, AudioKind};

impl Dashboard {
    pub(super) fn mixer_widget(&self, cx: &Context<Self>) -> impl IntoElement {
        if let Some(error) = &self.audio.issue {
            return quiet(error.clone()).into_any_element();
        }
        let view = cx.entity();
        if let Some(stream) = self
            .routing
            .as_ref()
            .and_then(|id| self.audio.channels.iter().find(|channel| &channel.id == id))
        {
            let back = bind(
                button("routing-back", None).h(r(28.)).child("Back"),
                move |window, cx| {
                    view.update(cx, |this, cx| {
                        this.focus_dashboard(window, cx);
                        this.routing = None;
                        cx.notify();
                    });
                },
            );
            let device_kind = if stream.id.kind == AudioKind::Playback {
                AudioKind::Output
            } else {
                AudioKind::Input
            };
            let devices: Vec<_> = self
                .visible_audio_devices()
                .filter(|channel| channel.id.kind == device_kind)
                .collect();
            return col()
                .flex_1()
                .overflow_hidden()
                .child(
                    row()
                        .justify_between()
                        .child(label("Route audio"))
                        .child(back),
                )
                .child(
                    col()
                        .id("route-devices")
                        .flex_1()
                        .overflow_y_scroll()
                        .py(r(4.))
                        .gap(r(6.))
                        .child(quiet(stream.label.clone()).w_full().flex_none().truncate())
                        .when(devices.is_empty(), |it| {
                            it.child(quiet("No compatible device available"))
                        })
                        .children(devices.into_iter().map(|device| {
                            let view = cx.entity();
                            let stream_id = stream.id.clone();
                            let device_id = device.id.clone();
                            let active = stream.device == Some(device.id.index);
                            bind(
                                button(
                                    (kind_key(device.id.kind), device.id.index),
                                    active.then_some(TEAL),
                                )
                                .justify_start()
                                .child(icon(
                                    if active { Icon::Check } else { Icon::Chevron },
                                    c(if active { TEAL } else { MUTED }),
                                    13.,
                                ))
                                .child(div().truncate().child(device.label.clone())),
                                move |window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.focus_dashboard(window, cx);
                                        this.send_audio(AudioAction::Route(
                                            stream_id.clone(),
                                            device_id.clone(),
                                        ));
                                        this.routing = None;
                                        cx.notify();
                                    })
                                },
                            )
                        })),
                )
                .into_any_element();
        }

        let tabs = row().gap(r(5.)).children(
            [
                (AudioKind::Playback, "Playback"),
                (AudioKind::Recording, "Recording"),
            ]
            .into_iter()
            .map(|(kind, text)| {
                let view = view.clone();
                bind(
                    button(text, (self.audio_kind == kind).then_some(VIOLET))
                        .h(r(28.))
                        .child(text),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.audio_kind = kind;
                            cx.notify();
                        })
                    },
                )
            }),
        );

        let streams: Vec<_> = self
            .audio
            .channels
            .iter()
            .filter(|channel| channel.id.kind == self.audio_kind)
            .collect();
        col()
            .flex_1()
            .overflow_hidden()
            .gap(r(6.))
            .child(tabs)
            .when(streams.is_empty(), |it| {
                it.child(empty(
                    Icon::Audio,
                    VIOLET,
                    if self.audio_kind == AudioKind::Playback {
                        "No playback streams"
                    } else {
                        "No recording streams"
                    },
                    "Active applications will appear here.",
                ))
            })
            .when(!streams.is_empty(), |it| {
                it.child(
                    col()
                        .id("streams")
                        .flex_1()
                        .overflow_y_scroll()
                        .py(r(4.))
                        .gap(r(6.))
                        .children(streams.into_iter().map(|channel| self.channel(channel, cx))),
                )
            })
            .into_any_element()
    }

    fn audio_levels(&self, channel: &AudioChannel, cx: &Context<Self>) -> impl IntoElement {
        const LIMIT: f32 = 150.;
        let view = cx.entity();
        let kind = channel.id.kind;
        let key = kind_key(kind);
        let scope =
            |control: &str| ElementId::Name(format!("{key}-{}-{control}", channel.id.index).into());
        let recording = matches!(kind, AudioKind::Input | AudioKind::Recording);
        let accent = if recording { BLUE } else { TEAL };
        let mute_icon = match (recording, channel.muted) {
            (true, true) => Icon::MicOff,
            (true, false) => Icon::Mic,
            (false, true) => Icon::SpeakerOff,
            (false, false) => Icon::Speaker,
        };
        let mute_id = channel.id.clone();
        let muted = channel.muted;
        let mute_button = bind(
            button(scope("mute"), muted.then_some(AMBER))
                .size(r(31.))
                .px_0()
                .child(icon(mute_icon, c(if muted { AMBER } else { TEXT }), 16.)),
            {
                let view = view.clone();
                move |_, cx| {
                    view.read(cx)
                        .send_audio(AudioAction::Mute(mute_id.clone(), !muted))
                }
            },
        );
        let volume_id = channel.id.clone();
        let slider_id = format!("{key}-{}-volume", channel.id.index);
        let shown_volume = self
            .gesture
            .borrow()
            .as_ref()
            .filter(|(id, _)| *id == slider_id)
            .map(|(_, fraction)| (fraction * LIMIT).round() as u16)
            .or(channel.volume);
        let volume = slider(
            slider_id,
            channel.volume.map_or(0., |value| f32::from(value) / LIMIT),
            if channel.muted { MUTED } else { accent },
            channel.volume.is_some(),
            self.gesture.clone(),
            5. / LIMIT,
            {
                let view = view.clone();
                move |value, cx| {
                    view.read(cx).send_audio(AudioAction::Volume(
                        volume_id.clone(),
                        (value * LIMIT).round() as u16,
                    ))
                }
            },
        );
        let percent = mono(shown_volume.map_or_else(|| "—".into(), |value| format!("{value}%")))
            .w(r(38.))
            .flex_none()
            .text_right()
            .text_size(r(12.))
            .text_color(c(if muted { AMBER } else { TEXT }));
        row()
            .gap(r(6.))
            .flex_1()
            .child(mute_button)
            .child(volume)
            .child(percent)
    }

    pub(super) fn channel(&self, channel: &AudioChannel, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let kind = channel.id.kind;
        let key = kind_key(kind);
        let scope =
            |control: &str| ElementId::Name(format!("{key}-{}-{control}", channel.id.index).into());
        let recording = matches!(kind, AudioKind::Input | AudioKind::Recording);
        let device = matches!(kind, AudioKind::Input | AudioKind::Output);
        let accent = if recording { BLUE } else { TEAL };
        let control: AnyElement = if device {
            if channel.is_default {
                chip("Default", accent).into_any_element()
            } else {
                let id = channel.id.clone();
                bind(
                    button(scope("default"), None)
                        .h(r(27.))
                        .child("Set default"),
                    move |_, cx| {
                        view.read(cx).send_audio(AudioAction::Default(id.clone()));
                    },
                )
                .into_any_element()
            }
        } else {
            let route = channel.id.clone();
            let route_name = self
                .audio
                .channels
                .iter()
                .find(|device| {
                    Some(device.id.index) == channel.device
                        && device.id.kind
                            == if recording {
                                AudioKind::Input
                            } else {
                                AudioKind::Output
                            }
                })
                .map(|device| device.label.clone())
                .unwrap_or_else(|| "Route".into());
            bind(
                button(scope("route"), None)
                    .h(r(27.))
                    .max_w(r(150.))
                    .child(div().truncate().child(route_name))
                    .child(icon(Icon::Chevron, c(MUTED), 12.)),
                move |window, cx| {
                    view.update(cx, |this, cx| {
                        this.focus_dashboard(window, cx);
                        this.routing = Some(route.clone());
                        cx.notify();
                    })
                },
            )
            .into_any_element()
        };

        let name = label(channel.label.clone()).truncate().text_color(c(INK));
        let meter = self.audio_levels(channel, cx);
        let surface = col()
            .flex_none()
            .bg(c(BACK).opacity(0.42))
            .border_1()
            .border_color(c(STROKE))
            .rounded(r(8.));

        surface
            .px(r(7.))
            .py(r(5.))
            .gap(r(3.))
            .child(
                row()
                    .gap(r(5.))
                    .child(
                        col()
                            .flex_1()
                            .child(name)
                            .when(device, |it| {
                                it.child(quiet(if recording { "Microphone" } else { "Output" }))
                            })
                            .when(!device && !channel.detail.is_empty(), |it| {
                                it.child(quiet(channel.detail.clone()).truncate())
                            }),
                    )
                    .child(control),
            )
            .child(meter)
    }
}

fn kind_key(kind: AudioKind) -> &'static str {
    match kind {
        AudioKind::Output => "output",
        AudioKind::Input => "input",
        AudioKind::Playback => "playback",
        AudioKind::Recording => "recording",
    }
}
