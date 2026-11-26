use gpui::{Context, Div, IntoElement, Styled, prelude::*};

use super::{Dashboard, Tooltip, audio_icon, channel_description, kind_key};
use crate::{
    model::{AudioChannel, AudioKind},
    ui::{icons::Icon, slider::SliderStyle, theme::*},
};

const STRIP_GAP: f32 = 6.;

impl Dashboard {
    pub(in crate::ui) fn volume_mixer_widget(
        &self,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        if let Some(issue) = &self.audio.issue {
            return theme
                .empty(
                    Icon::Audio,
                    AMBER,
                    "Audio server unavailable",
                    issue.clone(),
                )
                .into_any_element();
        }

        let stream_kind = self.volume_mixer_kind;
        let recording = stream_kind == AudioKind::Recording;
        let compact = height < 170.;
        let (device_kind, device_name, device_icon, app_icon, accent) = if recording {
            (
                AudioKind::Input,
                if compact { "Mic" } else { "Microphone" },
                Icon::Mic,
                Icon::Mic,
                BLUE,
            )
        } else {
            (
                AudioKind::Output,
                "System",
                Icon::Speaker,
                Icon::Music,
                AMBER,
            )
        };
        let default_device =
            |channel: &AudioChannel| channel.is_default && channel.id.kind == device_kind;
        let master = self
            .visible_audio_devices()
            .find(|channel| default_device(channel));
        let streams: Vec<_> = self
            .audio
            .channels
            .iter()
            .filter(|channel| channel.id.kind == stream_kind)
            .collect();
        let strip_width = (width * 0.4).clamp(66., 94.);

        let master = if let Some(channel) = master {
            mixer_strip(theme, strip_width, Some(accent)).child(self.mixer_channel(
                channel,
                device_name,
                device_icon,
                accent,
                compact,
                cx,
            ))
        } else {
            let hidden = self.audio.channels.iter().any(default_device);
            mixer_strip(theme, strip_width, Some(accent))
                .justify_center()
                .child(theme.label(device_name))
                .child(
                    theme
                        .quiet(if hidden {
                            "Default device hidden"
                        } else {
                            "No default device"
                        })
                        .text_center(),
                )
        };

        let apps = if streams.is_empty() {
            col()
                .flex_1()
                .items_center()
                .justify_center()
                .child(
                    theme
                        .quiet(if recording {
                            "No apps recording"
                        } else {
                            "No apps playing"
                        })
                        .text_center(),
                )
                .into_any_element()
        } else {
            let overflows =
                (streams.len() + 1) as f32 * strip_width + streams.len() as f32 * STRIP_GAP > width;
            split()
                .id(kind_key(stream_kind))
                .flex_1()
                .gap(r(STRIP_GAP))
                .when(overflows, |it| it.overflow_x_scroll().occlude())
                .children(streams.into_iter().map(|channel| {
                    mixer_strip(theme, strip_width, None).child(self.mixer_channel(
                        channel,
                        &channel.label,
                        app_icon,
                        accent,
                        compact,
                        cx,
                    ))
                }))
                .into_any_element()
        };

        col()
            .id("volume-mixer")
            .flex_1()
            .gap(r(STRIP_GAP))
            .child(self.stream_tabs(
                stream_kind,
                accent,
                |this, kind| this.volume_mixer_kind = kind,
                cx,
            ))
            .child(
                split()
                    .flex_1()
                    .min_h_0()
                    .gap(r(STRIP_GAP))
                    .child(master)
                    .child(apps),
            )
            .into_any_element()
    }

    fn mixer_channel(
        &self,
        channel: &AudioChannel,
        name: &str,
        fallback: Icon,
        accent: Color,
        compact: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let key = kind_key(channel.id.kind);
        let scope = |control: &str| format!("volume-mixer-{key}-{}-{control}", channel.id.index);
        let description = channel_description(channel);
        let symbol = audio_icon(
            channel,
            fallback,
            theme,
            accent,
            if compact { 16. } else { 28. },
        );
        let label = theme
            .label(name.to_owned())
            .min_w_0()
            .truncate()
            .when(compact, |it| it.text_size(r(12.)).line_height(r(16.)))
            .text_color(theme.color(INK));
        let heading = if compact {
            row().gap(r(2.)).justify_center().child(symbol).child(label)
        } else {
            col()
                .items_center()
                .gap(r(4.))
                .child(symbol)
                .child(label.w_full().text_center())
        };
        let (volume, percent) = self.audio_volume(
            channel,
            scope("volume"),
            SliderStyle::Fader(if channel.muted { MUTED } else { accent }),
            cx,
        );

        col()
            .flex_1()
            .w_full()
            .items_center()
            .gap(r(if compact { 2. } else { 6. }))
            .child(
                heading
                    .id(scope("label"))
                    .w_full()
                    .flex_none()
                    .tooltip(move |_, cx| cx.new(|_| Tooltip(description.clone().into())).into()),
            )
            .child(percent.when(compact, |it| it.line_height(r(16.))))
            .child(volume)
            .child(
                self.audio_mute(channel, scope("mute"), cx)
                    .w(r(40.))
                    .h(r(if compact { 26. } else { 31. })),
            )
    }
}

fn mixer_strip(theme: Theme, width: f32, accent: Option<Color>) -> Div {
    let (stroke, surface) = match accent {
        Some(accent) => {
            let color = theme.color(accent);
            (color.opacity(0.34), color.opacity(0.08))
        }
        None => (theme.color(STROKE), theme.color(BACK).opacity(0.42)),
    };
    col()
        .w(r(width))
        .flex_none()
        .items_center()
        .gap(r(4.))
        .p(r(6.))
        .rounded(r(8.))
        .border_1()
        .border_color(stroke)
        .bg(surface)
}
