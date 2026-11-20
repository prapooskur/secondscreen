use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, Styled, div, prelude::*};

use crate::model::{AudioChannel, AudioKind, DiscordAction, DiscordState, Playback};

use super::super::{
    Dashboard, Page,
    discord::discord_button,
    icons::Icon,
    media::album_art,
    monitor::{gib, graph, rate, storage_list},
    percent,
    settings::Section,
    theme::*,
};
use super::layout::{Kind, Placement};

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Media => "Now playing",
            Self::Audio => "Audio",
            Self::Discord => "Discord",
            Self::Memory => "Memory",
            Self::Network => "Network",
            Self::Storage => "Storage",
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Performance => "Processor and graphics load",
            Self::Media => "Track details and playback",
            Self::Audio => "Default devices and volume",
            Self::Discord => "Voice channel and controls",
            Self::Memory => "RAM usage over time",
            Self::Network => "Download and upload activity",
            Self::Storage => "Usage for mounted volumes",
        }
    }

    pub(super) fn page(self) -> Page {
        match self {
            Self::Performance | Self::Memory | Self::Network | Self::Storage => Page::System,
            Self::Media => Page::Media,
            Self::Audio => Page::Audio,
            Self::Discord => Page::Discord,
        }
    }

    pub(super) fn icon(self) -> Icon {
        match self {
            Self::Performance => Icon::Activity,
            Self::Media => Icon::Music,
            Self::Audio => Icon::Speaker,
            Self::Discord => Icon::Discord,
            Self::Memory => Icon::Memory,
            Self::Network => Icon::Network,
            Self::Storage => Icon::Disk,
        }
    }

    pub(super) fn accent(self) -> u32 {
        match self {
            Self::Performance => TEAL,
            Self::Media => 0xf2799c,
            Self::Audio | Self::Network => AMBER,
            Self::Discord | Self::Memory => VIOLET,
            Self::Storage => BLUE,
        }
    }
}

impl Dashboard {
    pub(super) fn widget_content(&self, tile: Placement, cx: &Context<Self>) -> AnyElement {
        let wide = tile.columns >= 2;
        let tall = tile.widget.rows >= 3;
        match tile.widget.kind {
            Kind::Performance => self.performance_widget(tall).into_any_element(),
            Kind::Media => self.media_widget(wide, tall, cx).into_any_element(),
            Kind::Audio => self.audio_widget(tall, cx).into_any_element(),
            Kind::Discord => self.discord_widget(wide, tall, cx).into_any_element(),
            Kind::Memory => self.memory_widget(tall).into_any_element(),
            Kind::Network => self.network_widget(tall).into_any_element(),
            Kind::Storage => storage_list("home-storage", self.visible_disks()).into_any_element(),
        }
    }

    fn performance_widget(&self, tall: bool) -> impl IntoElement {
        let gpu = self.metrics.gpu.as_ref();
        col()
            .flex_1()
            .gap(r(9.))
            .child(
                split()
                    .gap(r(12.))
                    .flex_none()
                    .child(metric("CPU", percent_text(self.metrics.cpu), TEAL, tall))
                    .child(div().w(r(1.)).flex_none().bg(c(STROKE)))
                    .child(metric(
                        "GPU",
                        percent_text(gpu.and_then(|gpu| gpu.load)),
                        BLUE,
                        tall,
                    )),
            )
            .child(graph(
                self.history.cpu.clone(),
                Some(self.history.gpu.clone()),
                100.,
                TEAL,
            ))
            .when(tall, |it| {
                it.child(
                    row()
                        .gap(r(10.))
                        .justify_between()
                        .flex_none()
                        .child(
                            quiet(if self.metrics.cpu_name.is_empty() {
                                "CPU history".into()
                            } else {
                                self.metrics.cpu_name.clone()
                            })
                            .truncate(),
                        )
                        .child(
                            quiet(self.metrics.temperature.map_or_else(
                                || "60 s".into(),
                                |degrees| format!("{degrees:.0} °C"),
                            ))
                            .flex_none()
                            .text_color(c(TEAL)),
                        ),
                )
            })
    }

    fn memory_widget(&self, tall: bool) -> impl IntoElement {
        let memory = percent(self.metrics.memory_used, self.metrics.memory_total);
        col()
            .flex_1()
            .gap(r(8.))
            .child(metric("RAM in use", percent_text(memory), VIOLET, tall))
            .child(graph(self.history.memory.clone(), None, 100., VIOLET))
            .when(memory.is_some(), |it| {
                it.child(
                    quiet(format!(
                        "{:.1} of {:.1} GiB used",
                        gib(self.metrics.memory_used),
                        gib(self.metrics.memory_total)
                    ))
                    .truncate()
                    .flex_none(),
                )
            })
    }

    fn network_widget(&self, tall: bool) -> impl IntoElement {
        let top = self
            .history
            .download
            .iter()
            .chain(&self.history.upload)
            .copied()
            .fold(65_536.0, f32::max)
            * 1.08;
        let network_rate = |label, amount, color| {
            let (number, unit) = if self.metrics.network_available {
                rate(amount)
            } else {
                ("—".into(), "")
            };
            col().flex_1().gap(r(4.)).child(quiet(label)).child(
                row()
                    .items_baseline()
                    .gap(r(4.))
                    .child(
                        mono(number)
                            .text_size(r(22.))
                            .text_color(c(color))
                            .truncate(),
                    )
                    .child(quiet(unit).whitespace_nowrap()),
            )
        };
        col()
            .flex_1()
            .gap(r(8.))
            .child(
                split()
                    .gap(r(8.))
                    .flex_none()
                    .child(network_rate("Download", self.metrics.download, AMBER))
                    .child(network_rate("Upload", self.metrics.upload, BLUE)),
            )
            .child(graph(
                self.history.download.clone(),
                Some(self.history.upload.clone()),
                top,
                AMBER,
            ))
            .when(tall, |it| {
                it.child(quiet(if self.metrics.network_available {
                    "Last 60 seconds"
                } else {
                    "No active interface"
                }))
            })
    }

    fn media_widget(&self, wide: bool, tall: bool, cx: &Context<Self>) -> AnyElement {
        let Some(player) = self.current_player() else {
            return message(
                if self.media.issue.is_some() {
                    "Media service unavailable"
                } else if !self.media.players.is_empty() {
                    "All media apps are hidden"
                } else {
                    "Nothing playing"
                },
                if self.media.players.is_empty() {
                    "Start playback in a compatible player."
                } else {
                    "Choose apps in Settings → Media apps."
                },
            )
            .into_any_element();
        };
        let track = player.track.as_ref();
        let art = track.and_then(|track| track.artwork.clone());
        let cover = if wide && tall {
            96.
        } else if tall {
            65.
        } else {
            38.
        };
        let (status, color) = match player.playback {
            Playback::Playing => ("Playing", TEAL),
            Playback::Paused => ("Paused", AMBER),
            Playback::Stopped => ("Stopped", MUTED),
        };

        col()
            .flex_1()
            .gap(r(if tall { 11. } else { 5. }))
            .child(
                row()
                    .min_h_0()
                    .flex_1()
                    .gap(r(10.))
                    .child(album_art(art, cover))
                    .child(
                        col()
                            .flex_1()
                            .gap(r(3.))
                            .when(tall, |it| {
                                it.child(row().gap(r(5.)).child(dot(color)).child(quiet(status)))
                            })
                            .child(
                                div()
                                    .text_size(r(if wide { 23. } else { 16. }))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(
                                        track
                                            .map(|track| track.title.clone())
                                            .unwrap_or_else(|| player.name.clone()),
                                    ),
                            )
                            .child(
                                quiet(
                                    track
                                        .filter(|track| !track.artist.is_empty())
                                        .map(|track| track.artist.clone())
                                        .unwrap_or_else(|| player.name.clone()),
                                )
                                .truncate(),
                            ),
                    ),
            )
            .when(tall, |it| it.child(self.media_progress(player, cx)))
            .child(self.media_transport(player, true, cx))
            .into_any_element()
    }

    fn audio_widget(&self, tall: bool, cx: &Context<Self>) -> AnyElement {
        let devices: Vec<_> = [AudioKind::Output, AudioKind::Input]
            .into_iter()
            .flat_map(|kind| {
                self.visible_audio_devices()
                    .filter(move |channel| channel.id.kind == kind)
            })
            .take(if tall { usize::MAX } else { 1 })
            .collect();
        if devices.is_empty() {
            return message(
                if self.audio.issue.is_some() {
                    "Audio server unavailable"
                } else {
                    "No audio devices shown"
                },
                "Choose devices in Settings → Audio devices.",
            )
            .into_any_element();
        }

        col()
            .id("home-audio-devices")
            .flex_1()
            .overflow_y_scroll()
            .child(
                col()
                    .gap(r(12.))
                    .flex_none()
                    .children(devices.into_iter().map(|device| {
                        let description = if device.id.kind == AudioKind::Output {
                            "Output"
                        } else {
                            "Microphone"
                        };
                        self.audio_widget_device(device, description, cx)
                    })),
            )
            .into_any_element()
    }

    fn audio_widget_device(
        &self,
        device: &AudioChannel,
        description: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        col()
            .gap(r(4.))
            .flex_none()
            .child(quiet(description))
            .child(
                div()
                    .text_size(r(15.))
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(device.label.clone()),
            )
            .child(self.audio_levels(device, cx))
    }

    fn discord_widget(&self, wide: bool, tall: bool, cx: &Context<Self>) -> AnyElement {
        if let DiscordState::Connected(voice) = &self.discord {
            let status = voice
                .channel
                .as_ref()
                .map(|channel| {
                    (
                        channel.name.clone(),
                        format!("{} in voice", channel.members.len()),
                    )
                })
                .unwrap_or_else(|| ("No active voice channel".into(), voice.account.clone()));
            return col()
                .flex_1()
                .justify_center()
                .gap(r(8.))
                .when(tall, |it| {
                    it.child(row().gap(r(6.)).child(dot(TEAL)).child(quiet("Connected")))
                })
                .child(
                    div()
                        .text_size(r(18.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(status.0),
                )
                .child(quiet(status.1).truncate())
                .child(
                    row()
                        .gap(r(6.))
                        .flex_none()
                        .child(discord_button(
                            "home-mute",
                            if voice.muted { Icon::MicOff } else { Icon::Mic },
                            if voice.muted { "Unmute" } else { "Mute" },
                            voice.muted.then_some(AMBER),
                            || DiscordAction::ToggleMute,
                            cx,
                        ))
                        .child(discord_button(
                            "home-deafen",
                            Icon::Headphones,
                            if voice.deafened { "Undeafen" } else { "Deafen" },
                            voice.deafened.then_some(AMBER),
                            || DiscordAction::ToggleDeafen,
                            cx,
                        ))
                        .when(wide && voice.channel.is_some(), |it| {
                            it.child(discord_button(
                                "home-leave",
                                Icon::Phone,
                                "Leave call",
                                Some(RED),
                                || DiscordAction::Leave,
                                cx,
                            ))
                        }),
                )
                .into_any_element();
        }

        let (heading, detail) = match &self.discord {
            DiscordState::Unconfigured => {
                ("Discord voice", "Set up Discord to see your voice channel.")
            }
            DiscordState::Authorizing => (
                "Waiting for Discord",
                "Approve the request in Discord desktop.",
            ),
            DiscordState::Error(error) => ("Discord connection failed", error.as_str()),
            _ => (
                "Discord disconnected",
                "Connect to see your channel and participants.",
            ),
        };
        let view = cx.entity();
        message(heading, detail)
            .when(!matches!(self.discord, DiscordState::Authorizing), |it| {
                it.child(row().child(bind(
                    button("home-discord-setup", Some(VIOLET)).child(
                        if matches!(self.discord, DiscordState::Unconfigured) {
                            "Set up Discord"
                        } else {
                            "Connect"
                        },
                    ),
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            if matches!(this.discord, DiscordState::Unconfigured) {
                                this.open_settings(Section::Discord, window, cx);
                            } else {
                                this.send_discord(DiscordAction::Connect);
                            }
                        })
                    },
                )))
            })
            .into_any_element()
    }
}

fn percent_text(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.0}%"))
}

fn metric(name: &'static str, value: String, color: u32, spacious: bool) -> Div {
    col()
        .flex_1()
        .gap(r(4.))
        .child(quiet(name).truncate())
        .child(
            mono(value)
                .text_size(r(if spacious { 33. } else { 22. }))
                .text_color(c(color))
                .truncate(),
        )
}

fn message(heading: &'static str, detail: &str) -> Div {
    col()
        .flex_1()
        .justify_center()
        .gap(r(8.))
        .child(
            div()
                .text_size(r(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(heading),
        )
        .child(quiet(detail.to_owned()).line_clamp(2))
}
