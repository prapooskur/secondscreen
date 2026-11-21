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

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};

use async_channel::Receiver;
use gpui::{
    Context, FocusHandle, FontWeight, IntoElement, KeyDownEvent, Render, SharedString, Window,
    prelude::*, px,
};

use crate::{
    config::Preferences,
    model::{
        AudioAction, AudioChannel, AudioKind, AudioState, DiscordAction, DiscordState, DiskUsage,
        MediaAction, MediaState, Metrics, Player, Update,
    },
    platform::Controls,
};
use icons::{Icon, icon};
use theme::*;

const HISTORY: usize = 60;
const CONTENT_X: f32 = 18.;
const CONTENT_Y: f32 = 16.;

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
    error: Option<(Instant, String)>,
}

impl Dashboard {
    pub fn new(
        controls: Controls,
        updates: Receiver<Update>,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            while let Ok(update) = updates.recv().await {
                if this
                    .update(cx, |this, cx| {
                        if this.apply(update) {
                            cx.notify();
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
            utilities: Default::default(),
            preferences,
            settings: settings::SettingsView::new(cx.focus_handle()),
            metrics: Metrics::default(),
            history: History::default(),
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

    fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Metrics(metrics) => {
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
                true
            }
            Update::Audio(state) => {
                self.audio = state;
                self.settings.open
                    || self.home.contains(home::Kind::Audio)
                    || self.home.contains(home::Kind::Mixer)
            }
            Update::Media(state) => {
                self.media = state;
                if self
                    .player
                    .as_ref()
                    .is_some_and(|id| !self.media.players.iter().any(|player| &player.id == id))
                {
                    self.player = None;
                }
                self.update_seek();
                self.settings.open || self.home.contains(home::Kind::Media)
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
                    return false;
                }
                self.pending_seek = None;
                self.error = Some((Instant::now(), error));
                true
            }
            Update::Discord(state) => {
                self.discord = state;
                self.home.contains(home::Kind::Discord) || self.home.contains(home::Kind::Viewer)
            }
            Update::ActionError(error) => {
                self.error = Some((Instant::now(), error));
                true
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
        self.metrics
            .storage
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
        if self.controls.audio.send(action).is_err() {
            log::error!("The audio worker stopped.");
        }
    }

    fn send_media(&self, id: String, action: MediaAction) {
        if self.controls.media.send((id, action)).is_err() {
            log::error!("The media worker stopped.");
        }
    }

    fn send_discord(&self, action: DiscordAction) {
        if self.controls.discord.send(action).is_err() {
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

    fn notice(&self) -> Option<impl IntoElement> {
        let (_, message) = self
            .error
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(9))?;
        Some(
            quiet(message.clone())
                .absolute()
                .right(r(18.))
                .bottom(r(18.))
                .max_w(gpui::relative(0.76))
                .px(r(12.))
                .py(r(8.))
                .rounded(r(9.))
                .border_1()
                .border_color(c(RED).opacity(0.45))
                .bg(c(HEADER))
                .text_color(c(RED)),
        )
    }

    fn toggle_fullscreen(&mut self, window: &mut Window) {
        window.toggle_fullscreen();
    }
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_viewers(cx);
        self.sync_utilities(cx);
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

        col()
            .relative()
            .px(r(CONTENT_X))
            .py(r(CONTENT_Y))
            .id("secondscreen")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .font_family("Inter")
            .text_size(r(14.))
            .text_color(c(INK))
            .size_full()
            .bg(c(BACK))
            .overflow_hidden()
            .child(home)
            .when(self.settings.open, |root| {
                root.child(self.settings_overlay(cx))
            })
            .children(self.notice())
    }
}

struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        label(self.0.clone())
            .text_color(c(INK))
            .font_weight(FontWeight::MEDIUM)
            .px(r(9.))
            .py(r(5.))
            .rounded(r(7.))
            .border_1()
            .border_color(c(STROKE))
            .bg(c(PANEL))
    }
}

fn window_control(
    description: &'static str,
    symbol: Icon,
    width: f32,
    action: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    bind(
        button(description, None)
            .w(r(width))
            .h(r(27.))
            .px_0()
            .child(icon(symbol, c(TEXT), 15.))
            .tooltip(move |_, cx| cx.new(|_| Tooltip(description.into())).into()),
        action,
    )
}

fn percent(used: u64, total: u64) -> Option<f32> {
    (total > 0).then(|| (100.0 * used as f64 / total as f64) as f32)
}
