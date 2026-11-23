use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, Styled, div, prelude::*};

use crate::model::AudioKind;

use super::super::{
    Dashboard,
    icons::Icon,
    media::album_art,
    monitor::{gib, graph, rate, storage_list},
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
            Self::Viewer => "Viewer",
            Self::WorldClock => "World clock",
            Self::Calendar => "Calendar",
            Self::Weather => "Weather / AQI",
            Self::Notifications => "Notifications",
            Self::Fps => "FPS",
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Performance => "Sensor chart, two sensors, or sensor list",
            Self::Media => "Track details and playback",
            Self::Audio => "Audio devices and volume",
            Self::Mixer => "App volume and mute",
            Self::Discord => "Voice channel and controls",
            Self::Memory => "RAM usage over time",
            Self::Network => "Download and upload activity",
            Self::Storage => "Drive capacity and usage",
            Self::Viewer => "Web, video, Twitch, and slideshows",
            Self::WorldClock => "Time and date across time zones",
            Self::Calendar => "Monthly calendar and today’s date",
            Self::Weather => "Today’s temperature, 10-day forecast, and air quality",
            Self::Notifications => "Recent desktop notifications",
            Self::Fps => "Current FPS, 1% low, and 0.1% low",
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
            Self::Viewer => Icon::Play,
            Self::WorldClock => Icon::Clock,
            Self::Calendar => Icon::Calendar,
            Self::Weather => Icon::Cloud,
            Self::Notifications => Icon::Bell,
            Self::Fps => Icon::Gauge,
        }
    }

    pub(super) fn accent(self) -> Color {
        match self {
            Self::Performance => TEAL,
            Self::Media => PINK,
            Self::Mixer | Self::Audio | Self::Network => AMBER,
            Self::Discord | Self::Memory => VIOLET,
            Self::Storage | Self::Viewer => BLUE,
            Self::WorldClock => TEAL,
            Self::Calendar => VIOLET,
            Self::Weather => AMBER,
            Self::Notifications => BLUE,
            Self::Fps => TEAL,
        }
    }
}

impl Dashboard {
    pub(super) fn widget_content(
        &self,
        tile: Placement,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let wide = tile.columns >= 2;
        match tile.widget.kind {
            Kind::Performance => self.performance_widget(tile.widget.id, height, cx),
            Kind::Media => self.media_widget(wide, tile.rows >= 3, height, cx),
            Kind::Audio => self.audio_widget(cx).into_any_element(),
            Kind::Mixer => self.mixer_widget(cx).into_any_element(),
            Kind::Discord => self.discord_widget(cx).into_any_element(),
            Kind::Memory => self.memory_widget(theme, height).into_any_element(),
            Kind::Network => self.network_widget(theme, height).into_any_element(),
            Kind::Storage => {
                storage_list(theme, "home-storage", self.visible_disks()).into_any_element()
            }
            Kind::WorldClock => self.world_clock(theme, height).into_any_element(),
            Kind::Calendar => self.calendar(height, cx).into_any_element(),
            Kind::Weather => self.weather_widget(theme, width, height).into_any_element(),
            Kind::Notifications => self.notification_widget(theme).into_any_element(),
            Kind::Fps => self.fps_widget(theme, width, height).into_any_element(),
            Kind::Viewer => div()
                .flex()
                .flex_1()
                .min_h_0()
                .children(self.viewers.get(&tile.widget.id).cloned())
                .into_any_element(),
        }
    }

    fn memory_widget(&self, theme: Theme, height: f32) -> impl IntoElement {
        let memory = percent(self.metrics.memory_used, self.metrics.memory_total);
        col()
            .flex_1()
            .gap(r(8.))
            .child(metric(
                theme,
                "RAM in use",
                percent_text(memory),
                VIOLET,
                height,
            ))
            .when(height >= 60., |it| {
                it.child(graph(
                    theme,
                    self.history.memory.clone(),
                    None,
                    100.,
                    VIOLET,
                ))
            })
            .when(height >= 105. && memory.is_some(), |it| {
                it.child(
                    theme
                        .quiet(format!(
                            "{:.1} of {:.1} GiB used",
                            gib(self.metrics.memory_used),
                            gib(self.metrics.memory_total)
                        ))
                        .truncate()
                        .flex_none(),
                )
            })
    }

    fn network_widget(&self, theme: Theme, height: f32) -> impl IntoElement {
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
                .child(theme.quiet(label).truncate())
                .child(
                    row()
                        .items_baseline()
                        .gap(r(4.))
                        .child(
                            mono(number)
                                .text_size(r(if height >= 75. { 22. } else { 14. }))
                                .text_color(theme.color(color))
                                .truncate(),
                        )
                        .child(theme.quiet(unit).whitespace_nowrap()),
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
                    theme,
                    self.history.download.clone(),
                    Some(self.history.upload.clone()),
                    top,
                    AMBER,
                ))
            })
            .when(height >= 105., |it| {
                it.child(theme.quiet(if self.metrics.network_available {
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
        let theme = Theme::current(cx);
        let Some(player) = self.current_player() else {
            return message(
                theme,
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

        col()
            .flex_1()
            .gap(r(if spacious { 11. } else { 5. }))
            .child(
                row()
                    .min_h_0()
                    .flex_1()
                    .gap(r(10.))
                    .when(height >= cover, |it| it.child(album_art(theme, art, cover)))
                    .child(
                        col()
                            .flex_1()
                            .gap(r(3.))
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
                                    theme
                                        .quiet(
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
                                    |it, track| {
                                        it.child(theme.quiet(track.album.clone()).truncate())
                                    },
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
        let theme = Theme::current(cx);
        let devices: Vec<_> = [AudioKind::Output, AudioKind::Input]
            .into_iter()
            .flat_map(|kind| {
                self.visible_audio_devices()
                    .filter(move |channel| channel.id.kind == kind)
            })
            .collect();
        if let Some(issue) = &self.audio.issue {
            return message(theme, "Audio server unavailable", issue).into_any_element();
        }
        if devices.is_empty() {
            return message(
                theme,
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

fn metric(theme: Theme, name: &'static str, value: String, color: Color, height: f32) -> Div {
    let presentation = if height >= 105. { col() } else { row() };
    presentation
        .flex_1()
        .gap(r(5.))
        .child(theme.quiet(name).truncate())
        .child(
            mono(value)
                .text_size(r(if height >= 170. { 33. } else { 21. }))
                .text_color(theme.color(color))
                .truncate(),
        )
}

fn message(theme: Theme, heading: &'static str, detail: &str) -> Div {
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
        .child(theme.quiet(detail.to_owned()).line_clamp(2))
}
