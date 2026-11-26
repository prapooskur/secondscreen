mod mixer;

use gpui::{Context, ElementId, IntoElement, ObjectFit, Stateful, Styled, img, prelude::*};

use super::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    slider::{SliderStyle, drag_value, slider},
    theme::*,
};
use crate::model::{AudioAction, AudioChannel, AudioKind};

impl Dashboard {
    pub(super) fn mixer_widget(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        if let Some(error) = &self.audio.issue {
            return theme.quiet(error.clone()).into_any_element();
        }
        let streams: Vec<_> = self
            .audio
            .channels
            .iter()
            .filter(|channel| channel.id.kind == self.audio_kind)
            .collect();
        col()
            .id("app-mixer")
            .flex_1()
            .overflow_hidden()
            .gap(r(6.))
            .child(self.stream_tabs(
                self.audio_kind,
                VIOLET,
                |this, kind| this.audio_kind = kind,
                cx,
            ))
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

    fn stream_tabs(
        &self,
        active: AudioKind,
        accent: Color,
        select: fn(&mut Self, AudioKind),
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity();
        row().flex_none().gap(r(5.)).children(
            [
                (AudioKind::Playback, "Playback"),
                (AudioKind::Recording, "Recording"),
            ]
            .map(|(kind, text)| {
                let view = view.clone();
                theme.bind(
                    theme
                        .button(text, (active == kind).then_some(accent))
                        .h(r(28.))
                        .child(text),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            select(this, kind);
                            cx.notify();
                        });
                    },
                )
            }),
        )
    }

    fn audio_mute(
        &self,
        channel: &AudioChannel,
        id: impl Into<ElementId>,
        cx: &Context<Self>,
    ) -> Stateful<gpui::Div> {
        let theme = Theme::current(cx);
        let view = cx.entity();
        let recording = matches!(channel.id.kind, AudioKind::Input | AudioKind::Recording);
        let muted = channel.muted;
        let mute_icon = match (recording, muted) {
            (true, true) => Icon::MicOff,
            (true, false) => Icon::Mic,
            (false, true) => Icon::SpeakerOff,
            (false, false) => Icon::Speaker,
        };
        let mute_id = channel.id.clone();
        let label = format!("Toggle mute for {}", channel.label);
        theme
            .bind(
                theme
                    .button(id, muted.then_some(AMBER))
                    .size(r(31.))
                    .px_0()
                    .child(icon(
                        mute_icon,
                        theme.color(if muted { AMBER } else { TEXT }),
                        16.,
                    )),
                move |_, cx| {
                    view.read(cx)
                        .send_audio(AudioAction::Mute(mute_id.clone(), !muted));
                },
            )
            .tooltip(move |_, cx| cx.new(|_| Tooltip(label.clone().into())).into())
    }

    fn audio_volume(
        &self,
        channel: &AudioChannel,
        slider_id: String,
        style: SliderStyle,
        cx: &Context<Self>,
    ) -> (impl IntoElement, gpui::Div) {
        const LIMIT: f32 = 150.;
        let theme = Theme::current(cx);
        let view = cx.entity();
        let volume_id = channel.id.clone();
        let shown_volume = drag_value(&self.gesture, &slider_id)
            .map(|fraction| (fraction * LIMIT).round() as u16)
            .or(channel.volume);
        let volume = slider(
            theme,
            slider_id,
            channel.volume.map_or(0., |value| f32::from(value) / LIMIT),
            style,
            channel.volume.is_some(),
            self.gesture.clone(),
            5. / LIMIT,
            move |value, cx| {
                view.read(cx).send_audio(AudioAction::Volume(
                    volume_id.clone(),
                    (value * LIMIT).round() as u16,
                ));
            },
        );
        let percent = mono(shown_volume.map_or_else(|| "—".into(), |value| format!("{value}%")))
            .flex_none()
            .text_size(r(12.))
            .text_color(theme.color(if channel.muted { AMBER } else { TEXT }));
        (volume, percent)
    }

    pub(super) fn channel(&self, channel: &AudioChannel, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity();
        let kind = channel.id.kind;
        let key = kind_key(kind);
        let scope = |control: &str| format!("{key}-{}-{control}", channel.id.index);
        let recording = matches!(kind, AudioKind::Input | AudioKind::Recording);
        let device = matches!(kind, AudioKind::Input | AudioKind::Output);
        let accent = if recording { BLUE } else { TEAL };
        let (volume, percent) = self.audio_volume(
            channel,
            scope("volume"),
            SliderStyle::Standard(if channel.muted { MUTED } else { accent }),
            cx,
        );
        let meter = row()
            .gap(r(6.))
            .flex_1()
            .child(self.audio_mute(channel, scope("mute"), cx))
            .child(volume)
            .child(percent.w(r(38.)).text_right());
        let surface = col()
            .flex_none()
            .bg(theme.color(BACK).opacity(0.42))
            .border_1()
            .border_color(theme.color(STROKE))
            .rounded(r(8.))
            .px(r(7.))
            .py(r(5.));
        if !device {
            let fallback = if recording { Icon::Mic } else { Icon::Music };
            let app_icon = row()
                .id(scope("app-icon"))
                .size(r(30.))
                .flex_none()
                .justify_center()
                .child(audio_icon(channel, fallback, theme, accent, 28.))
                .tooltip({
                    let description = channel_description(channel);
                    move |_, cx| cx.new(|_| Tooltip(description.clone().into())).into()
                });
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

fn channel_description(channel: &AudioChannel) -> String {
    if channel.detail.is_empty() {
        channel.label.clone()
    } else {
        format!("{} · {}", channel.label, channel.detail)
    }
}

fn audio_icon(
    channel: &AudioChannel,
    fallback: Icon,
    theme: Theme,
    accent: Color,
    size: f32,
) -> impl IntoElement {
    let fallback = move || icon(fallback, theme.color(accent), size - 4.).into_any_element();
    match &channel.icon {
        Some(path) => img(path.clone())
            .size(r(size))
            .object_fit(ObjectFit::Contain)
            .with_fallback(fallback)
            .into_any_element(),
        None => fallback(),
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
