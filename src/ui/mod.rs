mod audio;
mod credential_input;
mod discord;
mod home;
mod icons;
mod media;
mod monitor;
mod settings;
mod slider;
mod theme;

use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

use async_channel::Receiver;
use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, IntoElement, KeyDownEvent, MouseButton, Render,
    SharedString, Window, div, linear_color_stop, linear_gradient, prelude::*, px,
};

use crate::{
    config::Preferences,
    model::{
        AudioAction, AudioChannel, AudioId, AudioKind, AudioState, DiscordAction, DiscordState,
        DiskUsage, MediaAction, MediaState, Metrics, Player, Update,
    },
    platform::Controls,
};
use icons::{Icon, icon};
use theme::*;

const HISTORY: usize = 60;
const RAIL_WIDTH: f32 = 78.;
const CONTENT_X: f32 = 18.;
const CONTENT_Y: f32 = 16.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    System,
    Audio,
    Media,
    Discord,
    Settings,
}

impl Page {
    const ALL: [Self; 6] = [
        Self::Home,
        Self::System,
        Self::Audio,
        Self::Media,
        Self::Discord,
        Self::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::System => "System",
            Self::Audio => "Audio",
            Self::Media => "Media",
            Self::Discord => "Discord",
            Self::Settings => "Settings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Home => Icon::Home,
            Self::System => Icon::Activity,
            Self::Audio => Icon::Audio,
            Self::Media => Icon::Music,
            Self::Discord => Icon::Discord,
            Self::Settings => Icon::Settings,
        }
    }

    fn palette(self) -> (u32, u32, u32) {
        match self {
            Self::Home => (0x405972, 0x263749, 0xf2f7ff),
            Self::System => (0x31b987, 0x127453, 0xf3fff8),
            Self::Audio => (0xffbc65, 0xdb7624, 0x291b0e),
            Self::Media => (0xf25e84, 0xbf2b54, 0xfff6fa),
            Self::Discord => (0x828cff, 0x4b54d0, 0xffffff),
            Self::Settings => (0x879ab1, 0x516579, 0xffffff),
        }
    }
}

#[derive(Default)]
struct History {
    cpu: VecDeque<f32>,
    gpu: VecDeque<f32>,
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
    page: Page,
    home: home::Home,
    preferences: Preferences,
    settings: settings::SettingsView,
    metrics: Metrics,
    history: History,
    audio: AudioState,
    audio_kind: AudioKind,
    routing: Option<AudioId>,
    media: MediaState,
    player: Option<String>,
    pending_seek: Option<media::PendingSeek>,
    discord: DiscordState,
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
            page: Page::Home,
            home: home::Home::load(),
            preferences,
            settings: settings::SettingsView::default(),
            metrics: Metrics::default(),
            history: History::default(),
            audio: AudioState::default(),
            audio_kind: AudioKind::Playback,
            routing: None,
            media: MediaState::default(),
            player: None,
            pending_seek: None,
            discord: DiscordState::default(),
            controls,
            focus,
            gesture: Rc::new(RefCell::new(None)),
            error: None,
        }
    }

    fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Metrics(metrics) => {
                append(&mut self.history.cpu, metrics.cpu);
                if self.metrics.gpu.as_ref().map(|gpu| &gpu.key)
                    != metrics.gpu.as_ref().map(|gpu| &gpu.key)
                {
                    self.history.gpu.clear();
                }
                append(
                    &mut self.history.gpu,
                    metrics.gpu.as_ref().and_then(|gpu| gpu.load),
                );
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
                if self
                    .routing
                    .as_ref()
                    .is_some_and(|id| !self.audio.channels.iter().any(|channel| &channel.id == id))
                {
                    self.routing = None;
                }
                self.shows(Page::Audio)
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
                self.shows(Page::Media)
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
                self.shows(Page::Discord)
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
            .filter(|disk| !self.preferences.hidden_disks.contains_key(&disk.mount))
            .collect()
    }

    fn shows(&self, page: Page) -> bool {
        self.page == page
            || self.page == Page::Settings
            || (self.page == Page::Home && self.home.includes(page))
    }

    fn select(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != page {
            self.home.leave();
            self.page = page;
            self.routing = None;
            self.error = None;
            self.gesture.borrow_mut().take();
            self.focus_dashboard(window, cx);
            cx.notify();
        }
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
            "f11" => window.toggle_fullscreen(),
            "escape" if self.page == Page::Home && self.home.escape() => cx.notify(),
            "escape" if self.routing.take().is_some() => cx.notify(),
            "escape" if window.is_fullscreen() => window.toggle_fullscreen(),
            "tab" => {
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
            }
            "0" => self.select(Page::Home, window, cx),
            "1" => self.select(Page::System, window, cx),
            "2" => self.select(Page::Audio, window, cx),
            "3" => self.select(Page::Media, window, cx),
            "4" => self.select(Page::Discord, window, cx),
            "5" => self.select(Page::Settings, window, cx),
            _ => return,
        };
        cx.stop_propagation();
    }

    fn navigation(&self, window: &Window, height: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let fullscreen = window.is_fullscreen();
        let (tile_size, tile_gap) = if height < 412. {
            (39., 5.)
        } else if height < 486. {
            (46., 6.)
        } else {
            (54., 8.)
        };
        let view = cx.entity();
        let drag_space = || {
            div().flex_1().w_full().when(!fullscreen, |space| {
                space
                    .cursor_move()
                    .on_mouse_down(MouseButton::Left, |event, window, _| {
                        if event.click_count == 2 {
                            window.zoom_window();
                        } else {
                            window.start_window_move();
                        }
                    })
            })
        };

        let destinations =
            col()
                .items_center()
                .flex_none()
                .gap(r(tile_gap))
                .children(Page::ALL.into_iter().map(|page| {
                    let selected = self.page == page;
                    let (light, dark, foreground) = page.palette();
                    let view = view.clone();
                    bind(
                        row()
                            .id(page.label())
                            .size(r(tile_size))
                            .justify_center()
                            .rounded(r(16.))
                            .border_1()
                            .border_color(c(light).opacity(if selected { 0.76 } else { 0.0 }))
                            .bg(c(light).opacity(if selected { 0.15 } else { 0.0 }))
                            .hover(move |tile| tile.bg(c(light).opacity(0.22)))
                            .child(
                                row()
                                    .size(r(tile_size - 10.))
                                    .justify_center()
                                    .rounded(r(12.))
                                    .bg(linear_gradient(
                                        180.,
                                        linear_color_stop(c(light), 0.),
                                        linear_color_stop(c(dark), 1.),
                                    ))
                                    .child(icon(page.icon(), c(foreground), tile_size * 0.46)),
                            )
                            .tooltip(move |_, cx| cx.new(|_| Tooltip(page.label().into())).into()),
                        move |window, cx| {
                            view.update(cx, |this, cx| this.select(page, window, cx));
                        },
                    )
                }));

        let controls = col()
            .w_full()
            .items_center()
            .gap(r(6.))
            .pb(r(12.))
            .flex_none()
            .child(window_control(
                if fullscreen {
                    "Exit fullscreen (F11)"
                } else {
                    "Fullscreen (F11)"
                },
                if fullscreen {
                    Icon::Restore
                } else {
                    Icon::Fullscreen
                },
                44.,
                |window| window.toggle_fullscreen(),
            ))
            .when(!fullscreen, |controls| {
                controls.child(
                    row()
                        .gap(r(4.))
                        .child(window_control("Minimize", Icon::Minimize, 28., |window| {
                            window.minimize_window();
                        }))
                        .child(window_control("Close", Icon::Close, 28., |window| {
                            window.remove_window();
                        })),
                )
            });

        col()
            .w(r(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .items_center()
            .gap(r(8.))
            .pt(r(12.))
            .bg(c(HEADER))
            .border_r_1()
            .border_color(c(STROKE))
            .child(drag_space())
            .child(destinations)
            .child(drag_space())
            .child(controls)
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
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let scale = (f32::from(size.width) / 1400.)
            .min(f32::from(size.height) / 430.)
            .clamp(0.79, 1.72);
        window.set_rem_size(px(16. * scale));
        let width = f32::from(size.width) / scale;
        let height = f32::from(size.height) / scale;
        let page: AnyElement = match self.page {
            Page::Home => self
                .home_page(
                    width - RAIL_WIDTH - 2. * CONTENT_X,
                    height - 2. * CONTENT_Y,
                    scale,
                    cx,
                )
                .into_any_element(),
            Page::System => self.system_page().into_any_element(),
            Page::Audio => self.audio_page(cx).into_any_element(),
            Page::Media => self.media_page(cx).into_any_element(),
            Page::Discord => self.discord_page(cx).into_any_element(),
            Page::Settings => self.settings_page(cx).into_any_element(),
        };

        split()
            .id("secondscreen")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .font_family("Inter")
            .text_size(r(14.))
            .text_color(c(INK))
            .size_full()
            .bg(c(BACK))
            .overflow_hidden()
            .child(self.navigation(window, height, cx))
            .child(
                col()
                    .relative()
                    .flex_1()
                    .px(r(CONTENT_X))
                    .py(r(CONTENT_Y))
                    .child(page)
                    .children(self.notice()),
            )
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
    action: fn(&mut Window),
) -> impl IntoElement {
    bind(
        button(description, None)
            .w(r(width))
            .h(r(27.))
            .px_0()
            .child(icon(symbol, c(TEXT), 15.))
            .tooltip(move |_, cx| cx.new(|_| Tooltip(description.into())).into()),
        move |window, _| action(window),
    )
}

fn percent(used: u64, total: u64) -> Option<f32> {
    (total > 0).then(|| (100.0 * used as f64 / total as f64) as f32)
}
