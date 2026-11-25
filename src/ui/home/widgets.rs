use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, Styled, div, prelude::*};

use crate::model::AudioKind;

use super::super::{Dashboard, icons::Icon, media::album_art, theme::*};
use super::layout::{Kind, Placement};

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::SystemMonitor => "System monitor",
            Self::Media => "Now playing",
            Self::Audio => "Audio",
            Self::Mixer => "App mixer",
            Self::Discord => "Discord",
            Self::Viewer => "Viewer",
            Self::WorldClock => "World clock",
            Self::Calendar => "Calendar",
            Self::Weather => "Weather / AQI",
            Self::Notifications => "Notifications",
            Self::Fps => "FPS",
            Self::HomeAssistant => "Home Assistant",
            Self::QuickControls => "Quick controls",
        }
    }

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::SystemMonitor => "Hardware sensors, RAM, drives, and network",
            Self::Media => "Track details and playback",
            Self::Audio => "Audio devices and volume",
            Self::Mixer => "App volume and mute",
            Self::Discord => "Voice channel and controls",
            Self::Viewer => "Web, video, Twitch, and slideshows",
            Self::WorldClock => "Time and date across time zones",
            Self::Calendar => "Month view and events from your system calendar",
            Self::Weather => "Today’s temperature, 10-day forecast, and air quality",
            Self::Notifications => "Recent desktop notifications",
            Self::Fps => "Current FPS, 1% low, and 0.1% low",
            Self::HomeAssistant => "Smart home controls",
            Self::QuickControls => "Customizable controls and shortcuts",
        }
    }

    pub(super) fn icon(self) -> Icon {
        match self {
            Self::SystemMonitor => Icon::Activity,
            Self::Media => Icon::Music,
            Self::Audio => Icon::Speaker,
            Self::Mixer => Icon::Audio,
            Self::Discord => Icon::Discord,
            Self::Viewer => Icon::Play,
            Self::WorldClock => Icon::Clock,
            Self::Calendar => Icon::Calendar,
            Self::Weather => Icon::Cloud,
            Self::Notifications => Icon::Bell,
            Self::Fps => Icon::Gauge,
            Self::HomeAssistant => Icon::Home,
            Self::QuickControls => Icon::Grid,
        }
    }

    pub(super) fn accent(self) -> Color {
        match self {
            Self::SystemMonitor => TEAL,
            Self::Media => PINK,
            Self::Mixer | Self::Audio => AMBER,
            Self::Discord => VIOLET,
            Self::Viewer => BLUE,
            Self::WorldClock => TEAL,
            Self::Calendar => VIOLET,
            Self::Weather => AMBER,
            Self::Notifications => BLUE,
            Self::Fps => TEAL,
            Self::HomeAssistant => BLUE,
            Self::QuickControls => TEAL,
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
            Kind::SystemMonitor => self.system_monitor_widget(tile.widget.id, width, height, cx),
            Kind::Media => self.media_widget(wide, tile.rows >= 3, height, cx),
            Kind::Audio => self.audio_widget(cx).into_any_element(),
            Kind::Mixer => self.mixer_widget(cx).into_any_element(),
            Kind::Discord => self.discord_widget(cx).into_any_element(),
            Kind::WorldClock => self.world_clock(theme, height).into_any_element(),
            Kind::Calendar => self.calendar(theme, width, height, cx).into_any_element(),
            Kind::Weather => self.weather_widget(theme, width, height).into_any_element(),
            Kind::Notifications => self.notification_widget(theme).into_any_element(),
            Kind::Fps => self
                .fps_widget(tile.widget.id, theme, width, height)
                .into_any_element(),
            Kind::HomeAssistant => self
                .home_assistant_widget(tile.widget.id, height, cx)
                .into_any_element(),
            Kind::QuickControls => self
                .quick_controls_widget(tile.widget.id, width, height, cx)
                .into_any_element(),
            Kind::Viewer => div()
                .flex()
                .flex_1()
                .min_h_0()
                .children(self.viewers.get(&tile.widget.id).cloned())
                .into_any_element(),
        }
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
