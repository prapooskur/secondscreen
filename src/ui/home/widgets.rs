use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, Styled, div, prelude::*};

use crate::model::{AudioKind, Playback};

use super::super::{
    Dashboard,
    icons::Icon,
    media::album_art,
    monitor::{gib, graph, rate, storage_list, uptime},
    percent,
    theme::*,
};
use super::layout::{Kind, Placement};

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Media => "Now playing",
            Self::Audio => "Audio",
            Self::Mixer => "App mixer",
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
            Self::Audio => "Audio devices and volume",
            Self::Mixer => "App volume and stream routing",
            Self::Discord => "Voice channel and controls",
            Self::Memory => "RAM usage over time",
            Self::Network => "Download and upload activity",
            Self::Storage => "Drive capacity and usage",
        }
    }

    pub(super) fn icon(self) -> Icon {
        match self {
            Self::Performance => Icon::Activity,
            Self::Media => Icon::Music,
            Self::Audio => Icon::Speaker,
            Self::Mixer => Icon::Audio,
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
            Self::Mixer | Self::Audio | Self::Network => AMBER,
            Self::Discord | Self::Memory => VIOLET,
            Self::Storage => BLUE,
        }
    }
}

impl Dashboard {
    pub(super) fn widget_content(
        &self,
        tile: Placement,
        height: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let wide = tile.columns >= 2;
        match tile.widget.kind {
            Kind::Performance => self.performance_widget(height).into_any_element(),
            Kind::Media => self.media_widget(wide, tile.widget.rows >= 3, height, cx),
            Kind::Audio => self.audio_widget(cx).into_any_element(),
            Kind::Mixer => self.mixer_widget(cx).into_any_element(),
            Kind::Discord => self.discord_widget(cx).into_any_element(),
            Kind::Memory => self.memory_widget(height).into_any_element(),
            Kind::Network => self.network_widget(height).into_any_element(),
            Kind::Storage => storage_list("home-storage", self.visible_disks()).into_any_element(),
        }
    }

    fn performance_widget(&self, height: f32) -> impl IntoElement {
        let gpu = self.metrics.gpu.as_ref();
        col()
            .flex_1()
            .gap(r(9.))
            .child(
                split()
                    .gap(r(12.))
                    .flex_none()
                    .child(metric("CPU", percent_text(self.metrics.cpu), TEAL, height))
                    .child(div().w(r(1.)).flex_none().bg(c(STROKE)))
                    .child(metric(
                        "GPU",
                        percent_text(gpu.and_then(|gpu| gpu.load)),
                        BLUE,
                        height,
                    )),
            )
            .when(height >= 60., |it| {
                it.child(graph(
                    self.history.cpu.clone(),
                    Some(self.history.gpu.clone()),
                    100.,
                    TEAL,
                ))
            })
            .when(height >= 105., |it| {
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
                                || format!("{} threads", self.metrics.threads),
                                |degrees| format!("{degrees:.0} °C"),
                            ))
                            .flex_none()
                            .text_color(c(TEAL)),
                        ),
                )
            })
            .when(height >= 170., |it| {
                it.when_some(gpu, |it, gpu| {
                    it.child(
                        row()
                            .justify_between()
                            .gap(r(6.))
                            .child(quiet(gpu.name.clone()).truncate())
                            .when_some(gpu.vram, |it, (used, total)| {
                                it.child(
                                    quiet(format!("{:.1}/{:.1} GiB VRAM", gib(used), gib(total)))
                                        .flex_none(),
                                )
                            }),
                    )
                })
                .when_some(self.metrics.uptime, |it, seconds| {
                    it.child(
                        row()
                            .justify_between()
                            .gap(r(6.))
                            .child(quiet(self.metrics.host.clone()).truncate())
                            .child(quiet(format!("Uptime {}", uptime(seconds))).flex_none()),
                    )
                })
            })
    }

    fn memory_widget(&self, height: f32) -> impl IntoElement {
        let memory = percent(self.metrics.memory_used, self.metrics.memory_total);
        col()
            .flex_1()
            .gap(r(8.))
            .child(metric("RAM in use", percent_text(memory), VIOLET, height))
            .when(height >= 60., |it| {
                it.child(graph(self.history.memory.clone(), None, 100., VIOLET))
            })
            .when(height >= 105. && memory.is_some(), |it| {
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

    fn network_widget(&self, height: f32) -> impl IntoElement {
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
            let compact = height < 60.;
            let reading = if compact { row() } else { col() };
            reading
                .flex_1()
                .gap(r(4.))
                .child(quiet(label).truncate())
                .child(
                    row()
                        .items_baseline()
                        .gap(r(4.))
                        .child(
                            mono(number)
                                .text_size(r(if height >= 75. { 22. } else { 14. }))
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
            .when(height >= 75., |it| {
                it.child(graph(
                    self.history.download.clone(),
                    Some(self.history.upload.clone()),
                    top,
                    AMBER,
                ))
            })
            .when(height >= 105., |it| {
                it.child(quiet(if self.metrics.network_available {
                    "Last 60 seconds"
                } else {
                    "No active interface"
                }))
            })
    }

    fn media_widget(
        &self,
        wide: bool,
        show_progress: bool,
        height: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
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
        let spacious = height >= 170.;
        let show_progress = show_progress && height >= 78.;
        let inline_transport = height < if show_progress { 112. } else { 82. };
        let cover = if wide && spacious {
            96.
        } else if spacious {
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
            .gap(r(if spacious { 11. } else { 5. }))
            .child(
                row()
                    .min_h_0()
                    .flex_1()
                    .gap(r(10.))
                    .when(height >= cover, |it| it.child(album_art(art, cover)))
                    .child(
                        col()
                            .flex_1()
                            .gap(r(3.))
                            .when(spacious, |it| {
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
                            .when(height >= 45., |it| {
                                it.child(
                                    quiet(
                                        track
                                            .filter(|track| !track.artist.is_empty())
                                            .map(|track| track.artist.clone())
                                            .unwrap_or_else(|| player.name.clone()),
                                    )
                                    .truncate(),
                                )
                            })
                            .when(spacious && wide, |it| {
                                it.when_some(
                                    track.filter(|track| !track.album.is_empty()),
                                    |it, track| it.child(quiet(track.album.clone()).truncate()),
                                )
                            }),
                    )
                    .when(inline_transport && height >= 30., |it| {
                        it.child(self.media_transport(player, true, cx))
                    }),
            )
            .when(show_progress, |it| {
                it.child(self.media_progress(player, cx))
            })
            .when(!inline_transport, |it| {
                it.child(self.media_transport(player, !spacious, cx))
            })
            .into_any_element()
    }

    fn audio_widget(&self, cx: &Context<Self>) -> AnyElement {
        let devices: Vec<_> = [AudioKind::Output, AudioKind::Input]
            .into_iter()
            .flat_map(|kind| {
                self.visible_audio_devices()
                    .filter(move |channel| channel.id.kind == kind)
            })
            .collect();
        if let Some(issue) = &self.audio.issue {
            return message("Audio server unavailable", issue).into_any_element();
        }
        if devices.is_empty() {
            return message(
                "No audio devices shown",
                "Choose devices in Settings → Audio devices.",
            )
            .into_any_element();
        }

        col()
            .id("home-audio-devices")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(6.))
            .children(devices.into_iter().map(|device| self.channel(device, cx)))
            .into_any_element()
    }
}

fn percent_text(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.0}%"))
}

fn metric(name: &'static str, value: String, color: u32, height: f32) -> Div {
    let presentation = if height >= 105. { col() } else { row() };
    presentation
        .flex_1()
        .gap(r(5.))
        .child(quiet(name).truncate())
        .child(
            mono(value)
                .text_size(r(if height >= 170. { 33. } else { 21. }))
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
