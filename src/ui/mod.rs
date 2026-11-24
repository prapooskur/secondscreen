mod audio;
mod discord;
mod home;
mod icons;
mod media;
mod monitor;
mod settings;
mod slider;
mod text_input;
mod theme;
mod utilities;
mod viewer;

pub(crate) use icons::IconAssets;

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};

use async_channel::Receiver;
use futures_lite::future;
use gpui::{
    Context, FocusHandle, FontWeight, IntoElement, KeyDownEvent, Render, SharedString, Task,
    Window, prelude::*, px,
};

use crate::{
    config::Preferences,
    model::{
        AudioAction, AudioChannel, AudioKind, AudioState, DiscordAction, DiscordState, DiskUsage,
        MediaAction, MediaState, Metrics, Playback, Player, Update,
    },
    platform::Controls,
};
use theme::*;
use {home::Kind, settings::Section};

const HISTORY: usize = 60;
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const NOTICE_DURATION: Duration = Duration::from_secs(9);
const CONTENT_X: f32 = 18.;
const CONTENT_Y: f32 = 16.;

enum Refresh {
    None,
    Periodic,
    Immediate,
}

struct ErrorNotice {
    message: String,
    _dismissal: Task<()>,
}

#[derive(Default)]
struct History {
    sensors: HashMap<String, VecDeque<f32>>,
    memory: VecDeque<f32>,
    download: VecDeque<f32>,
    upload: VecDeque<f32>,
}

fn append(series: &mut VecDeque<f32>, value: Option<f32>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        if series.len() == HISTORY {
            series.pop_front();
        }
        series.push_back(value);
    } else {
        series.clear();
    }
}

pub struct Dashboard {
    home: home::Home,
    utilities: utilities::Utilities,
    preferences: Preferences,
    settings: settings::SettingsView,
    metrics: Metrics,
    history: History,
    storage: Vec<DiskUsage>,
    storage_active: bool,
    audio: AudioState,
    audio_kind: AudioKind,
    media: MediaState,
    player: Option<String>,
    pending_seek: Option<media::PendingSeek>,
    discord: DiscordState,
    viewers: HashMap<u32, gpui::Entity<viewer::Viewer>>,
    controls: Controls,
    focus: FocusHandle,
    gesture: slider::Gesture,
    error: Option<ErrorNotice>,
}

impl Dashboard {
    pub fn new(
        controls: Controls,
        updates: Receiver<Update>,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Theme::apply(preferences.theme, window.appearance(), cx);
        cx.observe_window_appearance(window, |this, window, cx| {
            Theme::apply(this.preferences.theme, window.appearance(), cx);
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let mut next_refresh = Instant::now();
            let mut pending = false;
            loop {
                let incoming = async { Some(updates.recv().await) };
                let update = if pending {
                    let delay = next_refresh.saturating_duration_since(Instant::now());
                    let timer = cx.background_executor().timer(delay);
                    future::or(
                        async {
                            timer.await;
                            None
                        },
                        incoming,
                    )
                    .await
                } else {
                    incoming.await
                };
                let update = match update {
                    Some(Ok(update)) => Some(update),
                    Some(Err(_)) => break,
                    None => None,
                };
                if this
                    .update(cx, |this, cx| {
                        let refresh =
                            update.map_or(Refresh::Periodic, |update| this.apply(update, cx));
                        let now = Instant::now();
                        match refresh {
                            Refresh::None => return,
                            Refresh::Periodic if now < next_refresh => pending = true,
                            Refresh::Periodic | Refresh::Immediate => {
                                pending = false;
                                next_refresh = now + REFRESH_INTERVAL;
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let focus = cx.focus_handle();
        #[cfg(feature = "ui-gpui")]
        window.focus(&focus);
        #[cfg(not(feature = "ui-gpui"))]
        window.focus(&focus, cx);
        Self {
            home: home::Home::load(),
            utilities: utilities::Utilities::new(),
            preferences,
            settings: settings::SettingsView::new(cx.focus_handle()),
            metrics: Metrics::default(),
            history: History::default(),
            storage: Vec::new(),
            storage_active: false,
            audio: AudioState::default(),
            audio_kind: AudioKind::Playback,
            media: MediaState::default(),
            player: None,
            pending_seek: None,
            discord: DiscordState::default(),
            viewers: HashMap::new(),
            controls,
            focus,
            gesture: Rc::new(RefCell::new(None)),
            error: None,
        }
    }

    fn showing_settings(&self, section: Section) -> bool {
        self.settings.open && self.settings.section == section
    }

    fn apply(&mut self, update: Update, cx: &mut Context<Self>) -> Refresh {
        match update {
            Update::Metrics(metrics) => {
                let redraw = [Kind::Performance, Kind::Memory, Kind::Network]
                    .into_iter()
                    .any(|kind| self.home.shows(kind));
                let mut previous = std::mem::take(&mut self.history.sensors);
                for sensor in &metrics.sensors {
                    let mut samples = previous.remove(&sensor.id).unwrap_or_default();
                    append(&mut samples, sensor.value);
                    self.history.sensors.insert(sensor.id.clone(), samples);
                }
                append(
                    &mut self.history.memory,
                    percent(metrics.memory_used, metrics.memory_total),
                );
                append(
                    &mut self.history.download,
                    metrics.network_available.then_some(metrics.download),
                );
                append(
                    &mut self.history.upload,
                    metrics.network_available.then_some(metrics.upload),
                );
                self.metrics = metrics;
                if redraw {
                    Refresh::Periodic
                } else {
                    Refresh::None
                }
            }
            Update::Storage(drives) => {
                if drives == self.storage {
                    return Refresh::None;
                }
                self.storage = drives;
                if self.storage_active {
                    Refresh::Periodic
                } else {
                    Refresh::None
                }
            }
            Update::Audio(state) => {
                if self.audio == state {
                    return Refresh::None;
                }
                self.audio = state;
                if self.showing_settings(Section::Audio)
                    || self.home.shows(Kind::Audio)
                    || self.home.shows(Kind::Mixer)
                {
                    Refresh::Immediate
                } else {
                    Refresh::None
                }
            }
            Update::Media(mut state) => {
                let show_media = self.home.shows(Kind::Media);
                let details_changed = (show_media || self.showing_settings(Section::Media))
                    && (self.media.issue != state.issue
                        || self.media.players.len() != state.players.len()
                        || self
                            .media
                            .players
                            .iter()
                            .zip(&state.players)
                            .any(|(previous, next)| !previous.same_details(next)));
                let refresh_progress = show_media
                    && self.home.shows_media_progress()
                    && self.current_player().is_some_and(|player| {
                        state
                            .players
                            .iter()
                            .find(|next| next.id == player.id)
                            .is_some_and(|next| {
                                (next.playback == Playback::Playing && next.position.is_some())
                                    || next.position.map(|position| position.reported)
                                        != player.position.map(|position| position.reported)
                            })
                    });
                self.follow_media_positions(&mut state);
                self.media = state;
                if self
                    .player
                    .as_ref()
                    .is_some_and(|id| !self.media.players.iter().any(|player| &player.id == id))
                {
                    self.player = None;
                }
                let had_seek = self.pending_seek.is_some();
                self.update_seek();
                if details_changed || (show_media && had_seek && self.pending_seek.is_none()) {
                    Refresh::Immediate
                } else if refresh_progress {
                    Refresh::Periodic
                } else {
                    Refresh::None
                }
            }
            Update::MediaSeekFailed {
                requested_at,
                error,
            } => {
                if self
                    .pending_seek
                    .as_ref()
                    .is_none_or(|seek| seek.requested_at != requested_at)
                {
                    return Refresh::None;
                }
                self.pending_seek = None;
                self.set_error(error, cx);
                Refresh::Immediate
            }
            Update::Discord(state) => {
                if self.discord == state {
                    return Refresh::None;
                }
                self.discord = state;
                if self.home.shows(Kind::Discord)
                    || self.home.shows(Kind::Viewer)
                    || self.showing_settings(Section::Discord)
                {
                    Refresh::Immediate
                } else {
                    Refresh::None
                }
            }
            Update::ActionError(error) => {
                self.set_error(error, cx);
                Refresh::Immediate
            }
        }
    }

    fn current_player(&self) -> Option<&Player> {
        self.player
            .as_deref()
            .and_then(|id| self.visible_players().find(|player| player.id == id))
            .or_else(|| {
                self.visible_players()
                    .find(|player| player.playback == crate::model::Playback::Playing)
            })
            .or_else(|| self.visible_players().next())
    }

    fn visible_players(&self) -> impl Iterator<Item = &Player> {
        self.media
            .players
            .iter()
            .filter(|player| !self.preferences.hidden_media.contains_key(&player.app_id))
    }

    fn visible_audio_devices(&self) -> impl Iterator<Item = &AudioChannel> {
        self.audio.channels.iter().filter(|channel| {
            settings::audio_device_key(channel)
                .is_some_and(|key| !self.preferences.hidden_audio.contains_key(&key))
        })
    }

    fn visible_disks(&self) -> Vec<&DiskUsage> {
        self.storage
            .iter()
            .filter(|disk| !self.preferences.hidden_drives.contains_key(&disk.id))
            .collect()
    }

    fn focus_dashboard(&self, window: &mut Window, _cx: &mut Context<Self>) {
        #[cfg(feature = "ui-gpui")]
        window.focus(&self.focus);
        #[cfg(not(feature = "ui-gpui"))]
        window.focus(&self.focus, _cx);
    }

    fn send_audio(&self, action: AudioAction) {
        if self.controls.audio.try_send(action).is_err() {
            log::error!("The audio worker stopped.");
        }
    }

    fn send_media(&self, id: String, action: MediaAction) {
        if self.controls.media.try_send((id, action)).is_err() {
            log::error!("The media worker stopped.");
        }
    }

    fn send_discord(&self, action: DiscordAction) {
        if self.controls.discord.try_send(action).is_err() {
            log::error!("The Discord worker stopped.");
        }
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.modifiers.control || key.modifiers.alt || key.modifiers.platform || event.is_held {
            return;
        }
        match key.key.as_str() {
            "f11" => self.toggle_fullscreen(window),
            "escape" if self.settings.open => self.close_settings(window, cx),
            "escape" if self.home.escape() => {
                self.focus_dashboard(window, cx);
                cx.notify();
            }
            "escape" if window.is_fullscreen() => self.toggle_fullscreen(window),
            "tab" => {
                let mut first_outside = None;
                loop {
                    #[cfg(feature = "ui-gpui")]
                    if key.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    #[cfg(not(feature = "ui-gpui"))]
                    if key.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    if !self.settings.open || self.settings.focus.contains_focused(window, cx) {
                        break;
                    }
                    // GPUI traverses the whole window. Skip dashboard controls under the overlay.
                    let current = window.focused(cx);
                    if current.is_none() || current == first_outside {
                        break;
                    }
                    first_outside = first_outside.or(current);
                }
            }
            _ => return,
        };
        cx.stop_propagation();
    }

    fn set_error(&mut self, message: String, cx: &mut Context<Self>) {
        let dismissal = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_DURATION).await;
            let _ = this.update(cx, |this, cx| {
                this.error = None;
                cx.notify();
            });
        });
        self.error = Some(ErrorNotice {
            message,
            _dismissal: dismissal,
        });
    }

    fn notice(&self, theme: Theme) -> Option<impl IntoElement> {
        let message = &self.error.as_ref()?.message;
        Some(
            theme
                .quiet(message.clone())
                .absolute()
                .right(r(18.))
                .bottom(r(18.))
                .max_w(gpui::relative(0.76))
                .px(r(12.))
                .py(r(8.))
                .rounded(r(9.))
                .border_1()
                .border_color(theme.color(RED).opacity(0.45))
                .bg(theme.color(HEADER))
                .text_color(theme.color(RED)),
        )
    }

    fn toggle_fullscreen(&mut self, window: &mut Window) {
        window.toggle_fullscreen();
    }
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let size = window.viewport_size();
        let scale = (f32::from(size.width) / 1400.)
            .min(f32::from(size.height) / 430.)
            .clamp(0.79, 1.72);
        window.set_rem_size(px(16. * scale));
        let width = f32::from(size.width) / scale;
        let height = f32::from(size.height) / scale;
        let home = self.home_page(
            width - 2. * CONTENT_X,
            height - 2. * CONTENT_Y,
            scale,
            window.is_fullscreen(),
            cx,
        );
        let storage_active =
            self.home.shows(Kind::Storage) || self.showing_settings(Section::Storage);
        if storage_active != self.storage_active {
            self.storage_active = storage_active;
            let _ = self.controls.storage.send(storage_active);
        }
        let artwork = self
            .home
            .contains(Kind::Media)
            .then(|| self.current_player())
            .flatten()
            .and_then(|player| player.track.as_ref())
            .and_then(|track| track.artwork.as_ref());
        media::retain_album_art(artwork, window, cx);

        col()
            .relative()
            .px(r(CONTENT_X))
            .py(r(CONTENT_Y))
            .id("secondscreen")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .font_family("Inter")
            .text_size(r(14.))
            .text_color(theme.color(INK))
            .size_full()
            .bg(theme.color(BACK))
            .overflow_hidden()
            .child(home)
            .when(self.settings.open, |root| {
                root.child(self.settings_overlay(cx))
            })
            .children(self.notice(theme))
    }
}

struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        theme
            .label(self.0.clone())
            .text_color(theme.color(INK))
            .font_weight(FontWeight::MEDIUM)
            .px(r(9.))
            .py(r(5.))
            .rounded(r(7.))
            .border_1()
            .border_color(theme.color(STROKE))
            .bg(theme.color(PANEL))
    }
}

fn percent(used: u64, total: u64) -> Option<f32> {
    (total > 0).then(|| (100.0 * used as f64 / total as f64) as f32)
}
