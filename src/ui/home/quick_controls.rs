mod setup;

use gpui::{AnyElement, Context, Div, ObjectFit, SharedString, Stateful, Styled, img, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
    config::ThemePreference,
    model::{
        AudioAction, AudioChannel, AudioKind, DiscordAction, DiscordState, MediaAction,
        NORMAL_VOLUME_PERCENT, Playback, Player,
    },
    platform::{
        desktop_apps::{self, DesktopApp},
        in_flatpak,
    },
    viewer::DISCORD_HOME,
};

use super::super::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    settings::Section,
    theme::*,
};

const GAP: f32 = 8.;
const MIN_SIDE: f32 = 34.;
const MAX_SIDE: f32 = 64.;
const VOLUME_STEP: i16 = 5;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum QuickAction {
    PreviousTrack,
    PlayPause,
    NextTrack,
    VolumeDown,
    SpeakerMute,
    VolumeUp,
    MicrophoneMute,
    AudioSettings,
    Fullscreen,
    SwitchPlayer,
    SwitchOutput,
    SwitchMicrophone,
    OpenDiscord,
    DiscordMute,
    DiscordDeafen,
    SwitchTheme,
    PreviousProfile,
    NextProfile,
    IntegrationStatus,
    LaunchApp(DesktopApp),
}

impl QuickAction {
    pub(super) const DEFAULT: &[Self] = &[
        Self::PreviousTrack,
        Self::PlayPause,
        Self::NextTrack,
        Self::VolumeDown,
        Self::SpeakerMute,
        Self::VolumeUp,
        Self::MicrophoneMute,
        Self::AudioSettings,
        Self::Fullscreen,
    ];

    const ADDITIONAL: &[Self] = &[
        Self::SwitchPlayer,
        Self::SwitchOutput,
        Self::SwitchMicrophone,
        Self::OpenDiscord,
        Self::DiscordMute,
        Self::DiscordDeafen,
        Self::SwitchTheme,
        Self::PreviousProfile,
        Self::NextProfile,
        Self::IntegrationStatus,
    ];

    pub(super) fn key(&self) -> &str {
        match self {
            Self::LaunchApp(app) => &app.id,
            _ => self.presentation().0,
        }
    }

    fn label(&self) -> SharedString {
        match self {
            Self::LaunchApp(app) => format!("Launch {}", app.name).into(),
            _ => self.presentation().0.into(),
        }
    }

    fn presentation(&self) -> (&'static str, Icon) {
        match self {
            Self::PreviousTrack => ("Previous track", Icon::Previous),
            Self::PlayPause => ("Play / pause", Icon::Play),
            Self::NextTrack => ("Next track", Icon::Next),
            Self::VolumeDown => ("Lower volume", Icon::SpeakerDown),
            Self::SpeakerMute => ("Speaker mute", Icon::Speaker),
            Self::VolumeUp => ("Raise volume", Icon::SpeakerUp),
            Self::MicrophoneMute => ("Microphone mute", Icon::Mic),
            Self::AudioSettings => ("Audio devices", Icon::Settings),
            Self::Fullscreen => ("Toggle fullscreen", Icon::Fullscreen),
            Self::SwitchPlayer => ("Switch media player", Icon::Music),
            Self::SwitchOutput => ("Switch audio output", Icon::Audio),
            Self::SwitchMicrophone => ("Switch microphone", Icon::Swap),
            Self::OpenDiscord => ("Open Discord", Icon::Discord),
            Self::DiscordMute => ("Discord mute", Icon::MicOff),
            Self::DiscordDeafen => ("Discord deafen", Icon::Headphones),
            Self::SwitchTheme => ("Switch light / dark theme", Icon::Moon),
            Self::PreviousProfile => ("Previous profile", Icon::Left),
            Self::NextProfile => ("Next profile", Icon::Chevron),
            Self::IntegrationStatus => ("Integration status", Icon::Activity),
            Self::LaunchApp(_) => ("Launch app", Icon::Grid),
        }
    }

    fn icon(&self, symbol: Icon, theme: Theme, color: Color, side: f32) -> AnyElement {
        if let Self::LaunchApp(app) = self
            && let Some(path) = &app.icon
        {
            return img(path.clone())
                .size(r(side))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || icon(symbol, theme.color(color), side).into_any_element())
                .into_any_element();
        }
        icon(symbol, theme.color(color), side).into_any_element()
    }
}

enum Action {
    Audio(AudioAction),
    Media(String, MediaAction),
    Settings(Section),
    Fullscreen,
    Player(String),
    OpenDiscord,
    DiscordMute,
    DiscordDeafen,
    Theme(ThemePreference),
    Profile(usize),
    LaunchApp(DesktopApp),
}

struct Control<'a> {
    kind: &'a QuickAction,
    label: SharedString,
    symbol: Icon,
    action: Option<Action>,
    accent: Option<Color>,
}

impl<'a> Control<'a> {
    fn new(kind: &'a QuickAction, action: Option<Action>) -> Self {
        Self {
            kind,
            label: kind.label(),
            symbol: kind.presentation().1,
            action,
            accent: None,
        }
    }

    fn displayed_as(mut self, label: impl Into<SharedString>, symbol: Icon) -> Self {
        self.label = label.into();
        self.symbol = symbol;
        self
    }

    fn accented(mut self, accent: Option<Color>) -> Self {
        self.accent = accent;
        self
    }

    fn button(self, side: f32, theme: Theme, cx: &Context<Dashboard>) -> Stateful<Div> {
        let label = self.label;
        let icon_side: f32 = if side >= 48. { 24. } else { 18. };
        let button = theme
            .button(format!("quick-control-{}", self.kind.key()), self.accent)
            .size(r(side))
            .px_0()
            .child(self.kind.icon(
                self.symbol,
                theme,
                self.accent.unwrap_or(TEXT),
                icon_side.min(side * 0.6),
            ))
            .tooltip(move |_, cx| cx.new(|_| Tooltip(label.clone())).into());
        let Some(action) = self.action else {
            return disabled(button);
        };
        let view = cx.entity();
        theme.bind(button, move |window, cx| match &action {
            Action::Audio(action) => view.read(cx).send_audio(action.clone()),
            Action::Media(id, action) => view.read(cx).send_media(id.clone(), action.clone()),
            Action::Settings(section) => {
                view.update(cx, |this, cx| this.open_settings(*section, window, cx))
            }
            Action::Fullscreen => view.update(cx, |this, _| this.toggle_fullscreen(window)),
            Action::Player(id) => view.update(cx, |this, cx| this.select_player(id, cx)),
            Action::OpenDiscord => {
                view.update(cx, |this, cx| this.open_discord(DISCORD_HOME, window, cx))
            }
            Action::DiscordMute => view.read(cx).send_discord(DiscordAction::ToggleMute),
            Action::DiscordDeafen => view.read(cx).send_discord(DiscordAction::ToggleDeafen),
            Action::Theme(preference) => {
                view.update(cx, |this, cx| this.set_theme(*preference, window, cx))
            }
            Action::Profile(index) => {
                view.update(cx, |this, cx| this.switch_profile(*index, window, cx))
            }
            Action::LaunchApp(app) => {
                let app = app.clone();
                view.update(cx, |_, cx| {
                    cx.spawn(async move |this, cx| {
                        if let Err(error) =
                            blocking::unblock(move || desktop_apps::launch(&app)).await
                        {
                            let _ = this.update(cx, |this, cx| {
                                this.set_error(format!("{error:#}"), cx);
                                cx.notify();
                            });
                        }
                    })
                    .detach();
                });
            }
        })
    }
}

impl Dashboard {
    pub(super) fn quick_controls_widget(
        &self,
        id: u32,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let viewport = col()
            .id(("quick-controls-grid", id as usize))
            .flex_1()
            .overflow_y_scroll();
        if width < 1. || height < 1. {
            return viewport;
        }
        let actions = self.home.layout().quick_actions();
        if actions.is_empty() {
            return viewport
                .items_center()
                .justify_center()
                .child(self.quick_controls_setup_button(cx));
        }

        let (columns, side) = grid_size(width, height, actions.len());
        let theme = Theme::current(cx);
        let grid = col()
            .gap(r(GAP))
            .children(actions.chunks(columns).map(|line| {
                row().gap(r(GAP)).flex_none().children(
                    line.iter()
                        .map(|action| self.quick_control(action, theme).button(side, theme, cx)),
                )
            }));

        viewport.child(
            col()
                .min_h(r(height))
                .flex_none()
                .items_center()
                .justify_center()
                .child(grid),
        )
    }

    fn quick_control<'a>(&self, kind: &'a QuickAction, theme: Theme) -> Control<'a> {
        use QuickAction::*;

        let player = self.current_player();
        let media = |command, enabled: fn(&Player) -> bool| {
            player
                .filter(|player| enabled(player))
                .map(|player| Action::Media(player.id.clone(), command))
        };
        let default_device = |kind| {
            self.visible_audio_devices()
                .find(|channel| channel.id.kind == kind && channel.is_default)
        };
        let voice = match &self.discord {
            DiscordState::Connected(voice) => Some(voice),
            _ => None,
        };

        match kind {
            PreviousTrack => Control::new(
                kind,
                media(MediaAction::Previous, |player| player.can_previous),
            ),
            NextTrack => Control::new(kind, media(MediaAction::Next, |player| player.can_next)),
            PlayPause => {
                let playing = player.is_some_and(|player| player.playback == Playback::Playing);
                let (label, symbol) = if playing {
                    ("Pause", Icon::Pause)
                } else {
                    ("Play", Icon::Play)
                };
                Control::new(
                    kind,
                    media(MediaAction::Toggle, Player::can_toggle_playback),
                )
                .displayed_as(label, symbol)
                .accented(playing.then_some(TEAL))
            }
            VolumeDown | VolumeUp => {
                let up = *kind == VolumeUp;
                let device = default_device(AudioKind::Output).filter(|channel| {
                    channel.volume.is_some_and(|volume| {
                        if up {
                            volume < NORMAL_VOLUME_PERCENT
                        } else {
                            volume > 0
                        }
                    })
                });
                let step = if up { VOLUME_STEP } else { -VOLUME_STEP };
                Control::new(
                    kind,
                    device.map(|channel| {
                        Action::Audio(AudioAction::AdjustVolume(channel.id.clone(), step))
                    }),
                )
            }
            SpeakerMute | MicrophoneMute => {
                let output = *kind == SpeakerMute;
                let device = default_device(if output {
                    AudioKind::Output
                } else {
                    AudioKind::Input
                });
                let muted = device.is_some_and(|channel| channel.muted);
                let (label, symbol) = match (output, muted) {
                    (true, true) => ("Unmute speakers", Icon::SpeakerOff),
                    (true, false) => ("Mute speakers", Icon::Speaker),
                    (false, true) => ("Unmute microphone", Icon::MicOff),
                    (false, false) => ("Mute microphone", Icon::Mic),
                };
                Control::new(
                    kind,
                    device
                        .map(|channel| Action::Audio(AudioAction::ToggleMute(channel.id.clone()))),
                )
                .displayed_as(label, symbol)
                .accented(muted.then_some(AMBER))
            }
            AudioSettings => Control::new(kind, Some(Action::Settings(Section::Audio))),
            Fullscreen => Control::new(kind, Some(Action::Fullscreen)),
            SwitchPlayer => {
                let next = next_choice(self.visible_players(), |candidate| {
                    player.is_some_and(|player| player.id == candidate.id)
                });
                if let Some(next) = next {
                    Control::new(kind, Some(Action::Player(next.id.clone())))
                        .displayed_as(format!("Switch to {}", next.name), Icon::Music)
                } else {
                    Control::new(kind, None)
                }
            }
            SwitchOutput | SwitchMicrophone => {
                let direction = if *kind == SwitchOutput {
                    AudioKind::Output
                } else {
                    AudioKind::Input
                };
                if let Some(next) = self.next_audio_device(direction) {
                    Control::new(
                        kind,
                        Some(Action::Audio(AudioAction::Default(next.id.clone()))),
                    )
                    .displayed_as(format!("Switch to {}", next.label), kind.presentation().1)
                } else {
                    Control::new(kind, None)
                }
            }
            OpenDiscord => Control::new(kind, Some(Action::OpenDiscord)),
            DiscordMute => {
                let muted = voice.is_some_and(|voice| voice.muted);
                Control::new(kind, voice.map(|_| Action::DiscordMute))
                    .displayed_as(
                        if muted {
                            "Unmute in Discord"
                        } else {
                            "Mute in Discord"
                        },
                        Icon::MicOff,
                    )
                    .accented(muted.then_some(AMBER))
            }
            DiscordDeafen => {
                let deafened = voice.is_some_and(|voice| voice.deafened);
                let (label, symbol) = if deafened {
                    ("Undeafen in Discord", Icon::HeadphonesOff)
                } else {
                    ("Deafen in Discord", Icon::Headphones)
                };
                Control::new(kind, voice.map(|_| Action::DiscordDeafen))
                    .displayed_as(label, symbol)
                    .accented(deafened.then_some(AMBER))
            }
            SwitchTheme => {
                let (preference, label, symbol) = match theme {
                    Theme::Dark => (ThemePreference::Light, "Use light theme", Icon::Sun),
                    Theme::Light => (ThemePreference::Dark, "Use dark theme", Icon::Moon),
                };
                Control::new(kind, Some(Action::Theme(preference))).displayed_as(label, symbol)
            }
            PreviousProfile | NextProfile => {
                let len = self.home.profiles.items.len();
                let step = if *kind == NextProfile { 1 } else { len - 1 };
                let index = (self.home.profiles.active + step) % len;
                Control::new(kind, (len > 1).then_some(Action::Profile(index)))
            }
            IntegrationStatus => Control::new(kind, Some(Action::Settings(Section::Integrations))),
            LaunchApp(app) => {
                if in_flatpak() {
                    Control::new(kind, None).displayed_as(desktop_apps::UNAVAILABLE, Icon::Grid)
                } else {
                    Control::new(kind, Some(Action::LaunchApp(app.clone())))
                }
            }
        }
    }

    fn next_audio_device(&self, kind: AudioKind) -> Option<&AudioChannel> {
        let mut devices = self
            .visible_audio_devices()
            .filter(|channel| channel.id.kind == kind)
            .collect::<Vec<_>>();
        // The mixer reports the default first. Use a stable order when it changes.
        devices.sort_by_cached_key(|channel| (channel.label.to_lowercase(), &channel.id.name));
        next_choice(devices.into_iter(), |channel| channel.is_default)
    }
}

fn next_choice<T>(
    mut choices: impl Iterator<Item = T>,
    is_current: impl Fn(&T) -> bool,
) -> Option<T> {
    let first = choices.next()?;
    if is_current(&first) {
        return choices.next();
    }
    if choices.by_ref().any(|choice| is_current(&choice)) {
        return choices.next().or(Some(first));
    }
    Some(first)
}

fn grid_size(width: f32, height: f32, count: usize) -> (usize, f32) {
    let min_side = MIN_SIDE.min(width);
    let fitting_columns = (((width + GAP) / (min_side + GAP)) as usize).clamp(1, count);
    // Use the widest square buttons that fit both dimensions, accounting for the final partial row.
    let (columns, side) = (1..=fitting_columns)
        .map(|columns| {
            let rows = count.div_ceil(columns);
            let across = (width + GAP) / columns as f32 - GAP;
            let down = (height + GAP) / rows as f32 - GAP;
            (columns, across.min(down))
        })
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .expect("the grid has at least one column");
    (columns, side.clamp(min_side, MAX_SIDE).floor())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_reflow_when_width_or_height_changes() {
        let count = QuickAction::DEFAULT.len();
        assert_eq!(grid_size(210., 160., count).0, 3);
        assert_eq!(grid_size(210., 82., count).0, 5);
        assert_eq!(grid_size(440., 82., count).0, 9);
        assert_eq!(grid_size(90., 220., count).0, 2);
    }

    #[test]
    fn short_tiles_keep_scrollable_buttons_at_a_usable_size() {
        let count = QuickAction::DEFAULT.len() + QuickAction::ADDITIONAL.len();
        let (columns, side) = grid_size(200., 36., count);
        assert_eq!(side, MIN_SIDE);
        assert!(columns as f32 * (side + GAP) - GAP <= 200.);
    }

    #[test]
    fn cycling_visits_each_choice_and_wraps() {
        let choices = [1, 2, 3];
        assert_eq!(next_choice(choices.into_iter(), |&item| item == 1), Some(2));
        assert_eq!(next_choice(choices.into_iter(), |&item| item == 2), Some(3));
        assert_eq!(next_choice(choices.into_iter(), |&item| item == 3), Some(1));
        assert_eq!(next_choice(choices.into_iter(), |&item| item == 4), Some(1));
        assert_eq!(next_choice([1].into_iter(), |&item| item == 1), None);
    }
}
