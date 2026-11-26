use gpui::{Context, Div, IntoElement, Styled, prelude::*};

use super::{Dashboard, Tooltip, audio_icon, channel_description, kind_key};
use crate::{
    model::{AudioChannel, AudioKind},
    ui::{home::MixerOrientation, icons::Icon, slider::SliderStyle, theme::*},
};

const STRIP_GAP: f32 = 6.;
const FADER_SPACING_SCALE: f32 = 0.66;

#[derive(Clone, Copy)]
struct ChannelLayout {
    vertical: bool,
    compact: bool,
}

impl Dashboard {
    pub(in crate::ui) fn volume_mixer_widget(
        &self,
        orientation: MixerOrientation,
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
        let vertical = orientation.is_vertical();
        let compact = if vertical {
            height < 170.
        } else {
            width < 260.
        };
        let channel_layout = ChannelLayout { vertical, compact };
        let (device_kind, device_icon, app_icon, accent) = if recording {
            (AudioKind::Input, Icon::Mic, Icon::Mic, BLUE)
        } else {
            (AudioKind::Output, Icon::Speaker, Icon::Music, AMBER)
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
        let strip_width = if compact { 66. } else { 65. };
        let strip = |accent| {
            mixer_strip(theme, accent).when(vertical, |it| it.w(r(strip_width)).items_center())
        };

        let master = if let Some(channel) = master {
            strip(Some(accent)).child(self.mixer_channel(
                channel,
                device_icon,
                accent,
                channel_layout,
                cx,
            ))
        } else {
            let hidden = self.audio.channels.iter().any(default_device);
            let heading = if vertical { col() } else { row().min_h(r(31.)) };
            strip(Some(accent)).justify_center().child(
                heading
                    .items_center()
                    .gap(r(4.))
                    .child(theme.icon_tile(device_icon, accent, 20.))
                    .child(
                        theme
                            .quiet(if hidden {
                                "Default device hidden"
                            } else {
                                "No default device"
                            })
                            .text_center(),
                    ),
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
            let overflows_horizontally = vertical
                && (streams.len() + 1) as f32 * strip_width + streams.len() as f32 * STRIP_GAP
                    > width;
            let bank = if vertical { split() } else { col() };
            bank.id((kind_key(stream_kind), usize::from(vertical)))
                .flex_1()
                .gap(r(STRIP_GAP))
                .when(overflows_horizontally, |it| {
                    it.overflow_x_scroll().occlude()
                })
                .when(!vertical, |it| {
                    it.overflow_y_scroll().restrict_scroll_to_axis()
                })
                .children(streams.into_iter().map(|channel| {
                    strip(None).child(self.mixer_channel(
                        channel,
                        app_icon,
                        accent,
                        channel_layout,
                        cx,
                    ))
                }))
                .into_any_element()
        };

        let body = if vertical { split() } else { col() };
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
                body.flex_1()
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
        fallback: Icon,
        accent: Color,
        layout: ChannelLayout,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let ChannelLayout { vertical, compact } = layout;
        let key = kind_key(channel.id.kind);
        let scope = |control: &str| format!("volume-mixer-{key}-{}-{control}", channel.id.index);
        let description = channel_description(channel);
        let icon_size = if compact { 20. } else { 26. };
        let symbol = row()
            .id(scope("icon"))
            .size(r(icon_size))
            .flex_none()
            .justify_center()
            .rounded(r(6.))
            .bg(theme.color(accent).opacity(0.11))
            .child(audio_icon(channel, fallback, theme, accent, icon_size - 6.))
            .tooltip(move |_, cx| cx.new(|_| Tooltip(description.clone().into())).into());
        let level_color = if channel.muted { MUTED } else { accent };
        let (volume, percent) = self.audio_volume(
            channel,
            scope("volume"),
            SliderStyle::Fader {
                accent: level_color,
                vertical,
            },
            cx,
        );
        let readout_color = if channel.volume.is_some() {
            level_color
        } else {
            MUTED
        };
        let controls = if vertical && !compact {
            col().items_center()
        } else {
            row()
        };
        let controls = controls
            .flex_none()
            .gap(r(if compact { 3. } else { STRIP_GAP }))
            .child(
                percent
                    .line_height(r(16.))
                    .text_color(theme.color(readout_color))
                    .when(!vertical, |it| it.w(r(32.)).text_right()),
            )
            .child(
                self.audio_mute(channel, scope("mute"), cx)
                    .when(compact, |it| it.size(r(24.))),
            );

        let gap = if vertical && compact { 2. } else { STRIP_GAP };
        let gap = r(gap * FADER_SPACING_SCALE);
        if vertical {
            col()
                .flex_1()
                .w_full()
                .items_center()
                .gap(gap)
                .child(symbol)
                .child(volume)
                .child(controls)
        } else {
            row().gap(gap).child(symbol).child(volume).child(controls)
        }
    }
}

fn mixer_strip(theme: Theme, accent: Option<Color>) -> Div {
    let (stroke, surface) = match accent {
        Some(accent) => {
            let color = theme.color(accent);
            (color.opacity(0.34), color.opacity(0.08))
        }
        None => (theme.color(STROKE), theme.color(BACK).opacity(0.42)),
    };
    col()
        .flex_none()
        .p(r(6. * FADER_SPACING_SCALE))
        .rounded(r(8.))
        .border_1()
        .border_color(stroke)
        .bg(surface)
}
