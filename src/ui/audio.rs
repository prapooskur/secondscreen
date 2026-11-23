use gpui::{Context, ElementId, IntoElement, ObjectFit, Styled, img, prelude::*};

use super::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    slider::slider,
    theme::*,
};
use crate::model::{AudioAction, AudioChannel, AudioKind};

impl Dashboard {
    pub(super) fn mixer_widget(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        if let Some(error) = &self.audio.issue {
            return theme.quiet(error.clone()).into_any_element();
        }
        let view = cx.entity();
        let tabs = row().gap(r(5.)).children(
            [
                (AudioKind::Playback, "Playback"),
                (AudioKind::Recording, "Recording"),
            ]
            .into_iter()
            .map(|(kind, text)| {
                let view = view.clone();
                theme.bind(
                    theme
                        .button(text, (self.audio_kind == kind).then_some(VIOLET))
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
                it.child(theme.empty(
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
        let theme = Theme::current(cx);
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
        let mute_button = theme.bind(
            theme
                .button(scope("mute"), muted.then_some(AMBER))
                .size(r(31.))
                .px_0()
                .child(icon(
                    mute_icon,
                    theme.color(if muted { AMBER } else { TEXT }),
                    16.,
                )),
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
            theme,
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
            .text_color(theme.color(if muted { AMBER } else { TEXT }));
        row()
            .gap(r(6.))
            .flex_1()
            .child(mute_button)
            .child(volume)
            .child(percent)
    }

    pub(super) fn channel(&self, channel: &AudioChannel, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity();
        let kind = channel.id.kind;
        let key = kind_key(kind);
        let scope =
            |control: &str| ElementId::Name(format!("{key}-{}-{control}", channel.id.index).into());
        let recording = matches!(kind, AudioKind::Input | AudioKind::Recording);
        let device = matches!(kind, AudioKind::Input | AudioKind::Output);
        let accent = if recording { BLUE } else { TEAL };
        let meter = self.audio_levels(channel, cx);
        let surface = col()
            .flex_none()
            .bg(theme.color(BACK).opacity(0.42))
            .border_1()
            .border_color(theme.color(STROKE))
            .rounded(r(8.))
            .px(r(7.))
            .py(r(5.));
        if !device {
            let description = if channel.detail.is_empty() {
                channel.label.clone()
            } else {
                format!("{} · {}", channel.label, channel.detail)
            };
            let fallback = if recording { Icon::Mic } else { Icon::Music };
            let app_icon = row()
                .id(scope("app-icon"))
                .size(r(30.))
                .flex_none()
                .justify_center()
                .child(match &channel.icon {
                    Some(path) => img(path.clone())
                        .size(r(28.))
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(move || {
                            icon(fallback, theme.color(accent), 24.).into_any_element()
                        })
                        .into_any_element(),
                    None => icon(fallback, theme.color(accent), 24.).into_any_element(),
                })
                .tooltip(move |_, cx| cx.new(|_| Tooltip(description.clone().into())).into());
            return surface
                .child(row().gap(r(8.)).child(app_icon).child(meter))
                .into_any_element();
        }
        let control = if channel.is_default {
            theme.chip("Default", accent).into_any_element()
        } else {
            let id = channel.id.clone();
            theme
                .bind(
                    theme
                        .button(scope("default"), None)
                        .h(r(27.))
                        .child("Set default"),
                    move |_, cx| {
                        view.read(cx).send_audio(AudioAction::Default(id.clone()));
                    },
                )
                .into_any_element()
        };

        let name = theme
            .label(channel.label.clone())
            .truncate()
            .text_color(theme.color(INK));

        surface
            .gap(r(3.))
            .child(
                row()
                    .gap(r(5.))
                    .child(col().flex_1().child(name).child(theme.quiet(if recording {
                        "Microphone"
                    } else {
                        "Output"
                    })))
                    .child(control),
            )
            .child(meter)
            .into_any_element()
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
