use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, Styled, div, prelude::*};

use crate::{config::FpsFeature, model::AudioKind};

use super::super::{Dashboard, icons::Icon, theme::*};
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
            Self::SystemMonitor => "Custom sensor bars, charts, and gauges",
            Self::Media => "Track details and playback",
            Self::Audio => "Audio devices and volume",
            Self::Mixer => "App volume and mute",
            Self::Discord => "Voice channel and controls",
            Self::Viewer => "Web, video, Twitch, and slideshows",
            Self::WorldClock => "Time and date across time zones",
            Self::Calendar => "Month view and events from your system calendar",
            Self::Weather => "Today’s temperature, 10-day forecast, and air quality",
            Self::Notifications => "Recent desktop notifications",
            Self::Fps => "FPS, 1% and 0.1% lows, and frame time",
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
        match tile.widget.kind {
            Kind::SystemMonitor => self.system_monitor_widget(tile.widget.id, width, height, cx),
            Kind::Media => self.media_widget(width, height, cx),
            Kind::Audio => self.audio_widget(cx).into_any_element(),
            Kind::Mixer => self.mixer_widget(cx).into_any_element(),
            Kind::Discord => self.discord_widget(cx).into_any_element(),
            Kind::WorldClock => self.world_clock(theme, height).into_any_element(),
            Kind::Calendar => self.calendar(theme, width, height, cx).into_any_element(),
            Kind::Weather => self.weather_widget(theme, width, height).into_any_element(),
            Kind::Notifications => self.notification_widget(theme).into_any_element(),
            Kind::Fps if self.home.fps_setup == Some(tile.widget.id) => self
                .fps_setup_menu(tile.widget.id, theme, cx)
                .into_any_element(),
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

    pub(super) fn fps_setup_button(&self, id: u32, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let open = self.home.fps_setup == Some(id);
        let view = cx.entity();
        theme.bind(
            theme
                .button(("fps-setup", id as usize), open.then_some(TEAL))
                .h(r(26.))
                .px(r(8.))
                .child(if open { "Done" } else { "Setup" }),
            move |window, cx| {
                view.update(cx, |this, cx| {
                    this.home.monitor_setup = None;
                    this.home.fps_setup = (!open).then_some(id);
                    this.focus_dashboard(window, cx);
                    cx.notify();
                });
            },
        )
    }

    fn fps_setup_menu(&self, id: u32, theme: Theme, cx: &Context<Self>) -> impl IntoElement {
        col()
            .id(("fps-setup-menu", id as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(4.))
            .children(FpsFeature::ALL.map(|feature| {
                let name = match feature {
                    FpsFeature::CurrentFps => "Current FPS",
                    FpsFeature::OnePercentLow => "1% low",
                    FpsFeature::PointOnePercentLow => "0.1% low",
                    FpsFeature::FrameTime => "Frame time",
                    FpsFeature::FpsGraph => "FPS graph",
                    FpsFeature::FrameTimeGraph => "Peak frame time graph",
                };
                let enabled = !self.preferences.hidden_fps_features.contains(&feature);
                let view = cx.entity();
                theme.bind(
                    theme
                        .button(("fps-feature", feature as usize), enabled.then_some(TEAL))
                        .h(r(26.))
                        .px(r(8.))
                        .gap(r(6.))
                        .justify_between()
                        .child(div().flex_1().min_w_0().truncate().child(name))
                        .child(if enabled { "On" } else { "Off" }),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            let mut preferences = this.preferences.clone();
                            if !preferences.hidden_fps_features.remove(&feature) {
                                preferences.hidden_fps_features.insert(feature);
                            }
                            this.save_preferences(preferences, cx);
                        });
                    },
                )
            }))
    }

    fn media_widget(&self, width: f32, height: f32, cx: &Context<Self>) -> AnyElement {
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
        self.media_player_widget(player, width, height, cx)
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
