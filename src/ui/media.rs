mod lyrics;

pub(super) use lyrics::MediaLyrics;

use std::{
    io::{Cursor, Read},
    sync::Arc,
    time::{Duration, Instant},
};

use image::{Frame, ImageBuffer, Rgba, imageops};

use crate::model::{Artwork, MediaAction, MediaState, Playback, Player, Track};
use gpui::{
    App, Asset, Context, Div, FontWeight, Global, Img, ImgResourceLoader, IntoElement, ObjectFit,
    RenderImage, Role, SharedString, Styled, Toggled, Window, div, img, prelude::*,
};

use super::{
    Dashboard,
    icons::{Icon, icon},
    slider::{SliderStyle, drag_value, slider},
    theme::*,
};

const SEEK_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_ALBUM_SIDE: u32 = 768;
const MAX_ALBUM_BYTES: usize = 8 * 1024 * 1024;
const TOUCH_TARGET: f32 = 56.; // About 44 logical pixels at the minimum dashboard scale.
const CONTROL_GAP: f32 = 8.;
const TRANSPORT_WIDTH: f32 = 3. * TOUCH_TARGET + 2. * CONTROL_GAP;
const SCRUBBER_HEIGHT: f32 = TOUCH_TARGET + 2.;
const MIN_SCRUBBER_WIDTH: f32 = 196.;
const MIN_TRACK_HEIGHT: f32 = 28.;
const MIN_MEDIA_TAB_WIDTH: f32 = 70.;
const MEDIA_TAB_PADDING: f32 = 6.;
const LYRICS_TOGGLE_HEIGHT: f32 = 28.;
const LYRICS_TOGGLE_WIDTH: f32 = 84.;
const ARTWORK_GAP: f32 = 6.;

pub(super) struct MediaLayout {
    pub(super) show_progress: bool,
    inline_progress: bool,
    track_height: f32,
}

impl MediaLayout {
    pub(super) fn new(width: f32, height: f32) -> Self {
        let stacked_controls = TOUCH_TARGET + CONTROL_GAP + SCRUBBER_HEIGHT;
        let stacked = height >= MIN_TRACK_HEIGHT + CONTROL_GAP + stacked_controls;
        let inline_progress = !stacked
            && height >= MIN_TRACK_HEIGHT + CONTROL_GAP + SCRUBBER_HEIGHT
            && width >= TRANSPORT_WIDTH + CONTROL_GAP + MIN_SCRUBBER_WIDTH;
        let controls_height = if stacked {
            stacked_controls
        } else if inline_progress {
            SCRUBBER_HEIGHT
        } else {
            TOUCH_TARGET
        };
        Self {
            show_progress: stacked || inline_progress,
            inline_progress,
            track_height: height - CONTROL_GAP - controls_height,
        }
    }

    pub(super) fn lyrics_height(&self) -> Option<f32> {
        (self.track_height >= LYRICS_TOGGLE_HEIGHT).then_some(self.track_height)
    }
}

pub(super) struct PendingSeek {
    player_id: String,
    track_id: String,
    position: i64,
    previous_position: Option<i64>,
    pub(super) requested_at: Instant,
}

impl PendingSeek {
    pub(super) fn matches(&self, player: &Player) -> bool {
        self.player_id == player.id
            && player
                .track
                .as_ref()
                .is_some_and(|track| self.track_id == track.id)
    }

    fn advance(player: &Player, elapsed: Duration) -> i64 {
        if player.playback == Playback::Playing {
            (elapsed.as_secs_f64() * player.rate * 1_000_000.) as i64
        } else {
            0
        }
    }

    fn displayed_position(&self, player: &Player, now: Instant) -> i64 {
        let elapsed = now.saturating_duration_since(self.requested_at);
        self.position
            .saturating_add(Self::advance(player, elapsed))
            .max(0)
    }

    fn confirmed(&self, player: &Player) -> bool {
        const TOLERANCE: i64 = 500_000;
        const ROUNDED_TOLERANCE: i64 = 2_000_000;
        let Some(position) = player.position else {
            return false;
        };
        let Some(elapsed) = position
            .sampled_at()
            .checked_duration_since(self.requested_at)
        else {
            return false;
        };
        let advance = Self::advance(player, elapsed);
        let near = |start: i64, margin: i64| {
            let end = start.saturating_add(advance);
            let lower = start.min(end).saturating_sub(margin);
            let upper = start.max(end).saturating_add(margin);
            (lower..=upper).contains(&position.reported)
        };
        // Accept coarser reports once they've clearly moved away from the old playhead.
        near(self.position, TOLERANCE)
            || (near(self.position, ROUNDED_TOLERANCE)
                && self
                    .previous_position
                    .is_some_and(|old| !near(old, ROUNDED_TOLERANCE)))
    }
}

impl Dashboard {
    pub(super) fn follow_media_positions(&self, state: &mut MediaState) {
        for next in &mut state.players {
            if next.playback != Playback::Playing {
                continue;
            }
            if let Some(seek) = self.pending_seek.as_ref().filter(|seek| seek.matches(next)) {
                if seek.confirmed(next)
                    && let Some(position) = &mut next.position
                {
                    position.start_from(seek.position, seek.requested_at);
                }
                continue;
            }
            let previous = self
                .media
                .players
                .iter()
                .find(|player| player.id == next.id);
            if let Some(previous) = previous
                && previous.playback == Playback::Playing
                && previous.rate == next.rate
                && previous.track == next.track
                && let (Some(position), Some(previous)) =
                    (next.position.as_mut(), previous.position)
            {
                position.follow(previous, next.rate);
            }
        }
    }

    pub(super) fn update_seek(&mut self) {
        if let Some(seek) = &self.pending_seek {
            let elapsed = seek.requested_at.elapsed();
            if elapsed >= SEEK_TIMEOUT
                || !self
                    .media
                    .players
                    .iter()
                    .any(|player| seek.matches(player) && !seek.confirmed(player))
            {
                self.pending_seek = None;
            }
        }
        let mut gesture = self.gesture.borrow_mut();
        if let Some((id, _)) = gesture.as_ref()
            && id.starts_with("seek:")
            && !self.current_player().is_some_and(|player| {
                player.track.as_ref().is_some_and(|track| {
                    seek_duration(player, track).is_some() && *id == seek_id(player, Some(track))
                })
            })
        {
            *gesture = None;
        }
    }

    fn seek_media(
        &mut self,
        player_id: &str,
        track_id: &str,
        position: i64,
        cx: &mut Context<Self>,
    ) {
        let requested_at = Instant::now();
        let previous_position = self
            .current_player()
            .filter(|player| player.id == player_id)
            .and_then(|player| player.position)
            .map(|position| position.reported);
        let action = MediaAction::Seek {
            track: track_id.to_owned(),
            position,
            requested_at,
        };
        if self
            .controls
            .media
            .try_send((player_id.to_owned(), action))
            .is_ok()
        {
            self.pending_seek = Some(PendingSeek {
                player_id: player_id.to_owned(),
                track_id: track_id.to_owned(),
                position,
                previous_position,
                requested_at,
            });
            self.error = None;
        } else {
            self.pending_seek = None;
            self.set_error("The media worker stopped.".into(), cx);
        }
        cx.notify();
    }

    pub(super) fn media_player_widget(
        &self,
        player: &Player,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let layout = MediaLayout::new(width, height);
        let track = player.track.as_ref();
        let track_height = layout.track_height;
        let show_toggle = layout.lyrics_height().is_some() && self.lyrics_for(player).is_some();
        let artwork_height = track_height
            - if show_toggle {
                LYRICS_TOGGLE_HEIGHT + ARTWORK_GAP
            } else {
                0.
            };
        let cover = artwork_height.min(width * 0.36).min(136.);
        let artwork = (cover >= MIN_TRACK_HEIGHT)
            .then(|| album_art(theme, track.and_then(|track| track.artwork.clone()), cover));
        let artwork_column = (artwork.is_some() || show_toggle).then(|| {
            col()
                .flex_none()
                .items_center()
                .gap(r(ARTWORK_GAP))
                .children(artwork)
                .children(
                    show_toggle
                        .then(|| self.lyrics_toggle(theme, cover.max(LYRICS_TOGGLE_WIDTH), cx)),
                )
        });
        let details = if show_toggle && self.media_lyrics.enabled {
            self.media_lyrics_panel(theme, track_height)
        } else {
            col()
                .gap(r(3.))
                .child(
                    div()
                        .text_size(r(if width >= 340. { 22. } else { 17. }))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(SharedString::from(track.map_or(
                            player.name.as_str(),
                            |track| {
                                if track.title.is_empty() {
                                    "Untitled track"
                                } else {
                                    track.title.as_str()
                                }
                            },
                        ))),
                )
                .when_some(track.filter(|_| track_height >= 48.), |it, track| {
                    let artist = track.artists.join(", ");
                    it.child(
                        theme
                            .quiet(if artist.is_empty() {
                                player.name.clone()
                            } else {
                                artist
                            })
                            .truncate(),
                    )
                    .when(track_height >= 84. && !track.album.is_empty(), |it| {
                        it.child(theme.quiet(track.album.clone()).truncate())
                    })
                })
        };
        let progress = layout.show_progress.then(|| {
            self.media_progress(player, cx)
                .when(layout.inline_progress, |it| it.flex_1())
        });
        let transport = self.media_transport(player, layout.inline_progress, cx);
        let controls = if layout.inline_progress {
            row()
                .gap(r(CONTROL_GAP))
                .children(progress)
                .child(transport)
        } else {
            col()
                .items_center()
                .gap(r(CONTROL_GAP))
                .children(progress)
                .child(transport)
        };

        col()
            .flex_1()
            .gap(r(CONTROL_GAP))
            .child(
                row()
                    .min_h_0()
                    .flex_1()
                    .gap(r(12.))
                    .children(artwork_column)
                    .child(details.flex_1()),
            )
            .child(controls.flex_none())
    }

    fn lyrics_toggle(&self, theme: Theme, width: f32, cx: &Context<Self>) -> impl IntoElement {
        let enabled = self.media_lyrics.enabled;
        let view = cx.entity();
        theme.bind(
            theme
                .button("lyrics-toggle", enabled.then_some(PINK))
                .w(r(width))
                .h(r(LYRICS_TOGGLE_HEIGHT))
                .px(r(6.))
                .role(Role::Switch)
                .aria_label("Lyrics")
                .aria_toggled(if enabled {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .child("Lyrics"),
            move |_, cx| {
                view.update(cx, |this, cx| {
                    this.media_lyrics.enabled = !this.media_lyrics.enabled;
                    cx.notify();
                });
            },
        )
    }

    fn displayed_media_position(&self, player: &Player, now: Instant) -> Option<i64> {
        self.pending_seek
            .as_ref()
            .filter(|seek| seek.matches(player))
            .map(|seek| seek.displayed_position(player, now))
            .or_else(|| {
                let position = player.position?;
                Some(if player.playback == Playback::Playing {
                    position.at(now, player.rate)
                } else {
                    position.reported
                })
            })
    }

    fn media_progress(&self, player: &Player, cx: &Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let track = player.track.as_ref();
        let length = track.and_then(|track| track.duration);
        let slider_id = seek_id(player, track);
        let current = self.displayed_media_position(player, Instant::now());
        let progress = current
            .zip(length)
            .map_or(0., |(at, total)| (at as f32 / total as f32).clamp(0., 1.));
        let enabled = track
            .and_then(|track| seek_duration(player, track))
            .is_some();
        let position = drag_value(&self.gesture, &slider_id)
            .and_then(|ratio| length.map(|length| seek_position(ratio, length)))
            .or(current)
            .map(|at| length.map_or(at, |total| at.min(total)));
        let seek = {
            let id = player.id.clone();
            let track_id = track.map(|track| track.id.clone()).unwrap_or_default();
            let view = cx.entity();
            slider(
                theme,
                slider_id,
                progress,
                SliderStyle::Scrubber {
                    accent: BLUE,
                    height: SCRUBBER_HEIGHT,
                },
                enabled,
                self.gesture.clone(),
                0.02,
                move |fraction, cx| {
                    view.update(cx, |this, cx| {
                        if let Some(length) = this
                            .current_player()
                            .filter(|player| player.id == id)
                            .and_then(|player| {
                                let track =
                                    player.track.as_ref().filter(|track| track.id == track_id)?;
                                seek_duration(player, track)
                            })
                        {
                            this.seek_media(&id, &track_id, seek_position(fraction, length), cx);
                        }
                    });
                },
            )
        };

        let timestamp = |micros: Option<i64>| {
            mono(micros.map(timecode).unwrap_or_else(|| "—:—".to_owned()))
                .text_size(r(12.))
                .whitespace_nowrap()
                .text_color(theme.color(TEXT))
        };
        row().relative().w_full().child(seek).child(
            row()
                .absolute()
                .top(r(4.))
                .left(r(10.))
                .right(r(10.))
                .justify_between()
                .child(timestamp(position))
                .child(timestamp(length)),
        )
    }

    fn media_transport(
        &self,
        player: &Player,
        inline: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let playing = player.playback == Playback::Playing;
        let button = |id, label, symbol, action: MediaAction, enabled, accent| {
            let label = SharedString::from(label);
            let control = theme
                .button(id, accent)
                .flex_1()
                .min_w(r(TOUCH_TARGET))
                .h(r(TOUCH_TARGET))
                .px_0()
                .rounded(r(12.))
                .child(icon(
                    symbol,
                    theme.color(accent.unwrap_or(TEXT)),
                    if accent.is_some() { 26. } else { 21. },
                ))
                .tooltip(move |_, cx| cx.new(|_| super::Tooltip(label.clone())).into());
            if enabled {
                let view = cx.entity();
                let id = player.id.clone();
                let pressed = accent.map_or(theme.color(HOVER), |accent| {
                    theme.color(accent).opacity(0.33)
                });
                theme.bind(control.active(move |it| it.bg(pressed)), move |_, cx| {
                    view.read(cx).send_media(id.clone(), action.clone());
                })
            } else {
                disabled(control)
            }
        };

        row()
            .w_full()
            .max_w(r(384.))
            .when(inline, |it| it.w(r(TRANSPORT_WIDTH)))
            .gap(r(CONTROL_GAP))
            .flex_none()
            .child(button(
                "previous",
                "Previous track",
                Icon::Previous,
                MediaAction::Previous,
                player.can_previous,
                None,
            ))
            .child(button(
                "play-pause",
                if playing { "Pause" } else { "Play" },
                if playing { Icon::Pause } else { Icon::Play },
                MediaAction::Toggle,
                player.can_toggle_playback(),
                Some(TEAL),
            ))
            .child(button(
                "next",
                "Next track",
                Icon::Next,
                MediaAction::Next,
                player.can_next,
                None,
            ))
    }

    pub(super) fn media_tabs_height(&self) -> f32 {
        if self.visible_players().nth(1).is_some() {
            TOUCH_TARGET
        } else {
            0.
        }
    }

    pub(super) fn media_tabs(&self, width: f32, cx: &Context<Self>) -> Option<Div> {
        let active = self.current_player()?;
        let players: Vec<_> = self.visible_players().collect();
        if players.len() < 2 {
            return None;
        }
        let theme = Theme::current(cx);
        let view = cx.entity();
        let overflow = players.len() as f32 * MIN_MEDIA_TAB_WIDTH + 2. * MEDIA_TAB_PADDING > width;
        let tabs = players.into_iter().map(|player| {
            let selected = player.id == active.id;
            let id = player.id.clone();
            let view = view.clone();
            let name = SharedString::from(player.name.clone());
            theme.bind(
                row()
                    .id(SharedString::from(id.clone()))
                    .flex_1()
                    .min_w(r(MIN_MEDIA_TAB_WIDTH))
                    .max_w(r(144.))
                    .h_full()
                    .justify_center()
                    .px(r(5.))
                    .border_b(r(2.))
                    .border_color(theme.color(HEADER).opacity(0.))
                    .text_size(r(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.color(TEXT))
                    .active(move |it| it.bg(theme.color(PINK).opacity(0.24)))
                    .hover(move |it| {
                        it.bg(if selected {
                            theme.color(PINK).opacity(0.2)
                        } else {
                            theme.color(HOVER)
                        })
                    })
                    .when(selected, |it| {
                        it.bg(theme.color(PINK).opacity(0.12))
                            .border_color(theme.color(PINK))
                            .text_color(theme.color(PINK))
                    })
                    .child(div().min_w_0().truncate().child(name.clone()))
                    .tooltip(move |_, cx| cx.new(|_| super::Tooltip(name.clone())).into()),
                move |_, cx| view.update(cx, |this, cx| this.select_player(&id, cx)),
            )
        });
        Some(
            row()
                .h(r(TOUCH_TARGET))
                .flex_none()
                .px(r(MEDIA_TAB_PADDING))
                .rounded_t(r(13.))
                .bg(theme.color(HEADER))
                .border_b_1()
                .border_color(theme.color(STROKE))
                .child(
                    row()
                        .id("media-tabs")
                        .flex_1()
                        .h_full()
                        .when(overflow, |it| it.overflow_x_scroll().occlude())
                        .children(tabs),
                ),
        )
    }

    pub(super) fn select_player(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.player.as_deref() != Some(id) {
            self.player = Some(id.to_owned());
            self.update_seek();
            cx.notify();
        }
    }
}

fn seek_id(player: &Player, track: Option<&Track>) -> String {
    let track_id = track.map_or("", |track| track.id.as_str());
    format!("seek:{}:{track_id}", player.id)
}

fn seek_duration(player: &Player, track: &Track) -> Option<i64> {
    track
        .duration
        .filter(|&length| player.can_seek && length > 0 && track.id.starts_with('/'))
}

fn seek_position(fraction: f32, duration: i64) -> i64 {
    // An MPRIS SetPosition at or beyond the duration is ignored by the player.
    ((f64::from(fraction) * duration as f64).round() as i64).clamp(0, duration - 1)
}

fn timecode(micros: i64) -> String {
    let seconds = micros.max(0) / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn album_art(theme: Theme, artwork: Option<Artwork>, side: f32) -> impl IntoElement {
    let radius = 10.;
    let art = artwork.map(|artwork| {
        img(move |window: &mut Window, cx: &mut App| {
            let pixels = side * (f32::from(window.rem_size()) / 16.) * window.scale_factor();
            let source = AlbumSource {
                artwork: artwork.clone(),
                pixels: (pixels.ceil() as u32).clamp(1, MAX_ALBUM_SIDE),
            };
            if cx.default_global::<AlbumCache>().0.as_ref() != Some(&source) {
                release_album_art(window, cx);
                cx.default_global::<AlbumCache>().0 = Some(source.clone());
            }
            window
                .use_asset::<AlbumThumbnail>(&source, cx)
                .flatten()
                .map(Ok)
        })
        .absolute()
        .inset_0()
        .size_full()
        .rounded(r(radius - 1.))
        .object_fit(ObjectFit::Fill)
    });

    row()
        .size(r(side))
        .flex_none()
        .relative()
        .overflow_hidden()
        .justify_center()
        .rounded(r(radius))
        .border(r(1.))
        .border_color(theme.color(STROKE))
        .bg(theme.color(ARTWORK_BACK))
        .child(
            row()
                .size(r(side * 0.675))
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(theme.color(BLUE).opacity(0.23))
                .child(
                    row()
                        .size(r(side * 0.496))
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(theme.color(BLUE).opacity(0.29))
                        .child(icon(Icon::Music, theme.color(BLUE), side * 0.22)),
                ),
        )
        .children(art)
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct AlbumSource {
    artwork: Artwork,
    pixels: u32,
}

// Only one media widget can appear in the current profile.
#[derive(Default)]
struct AlbumCache(Option<AlbumSource>);
impl Global for AlbumCache {}

pub(super) fn retain_album_art(artwork: Option<&Artwork>, window: &mut Window, cx: &mut App) {
    if cx
        .try_global::<AlbumCache>()
        .and_then(|cache| cache.0.as_ref())
        .is_some_and(|source| Some(&source.artwork) != artwork)
    {
        release_album_art(window, cx);
    }
}

fn release_album_art(window: &mut Window, cx: &mut App) {
    if let Some(source) = cx.default_global::<AlbumCache>().0.take() {
        let image = window.get_asset::<AlbumThumbnail>(&source, cx).flatten();
        cx.remove_asset::<AlbumThumbnail>(&source);
        if let Some(image) = image {
            cx.drop_image(image, Some(window));
        }
    }
}

struct AlbumThumbnail;
impl Asset for AlbumThumbnail {
    type Source = AlbumSource;
    type Output = Option<Arc<RenderImage>>;

    fn load(
        source: AlbumSource,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let load: futures_lite::future::Boxed<_> = match source.artwork {
            Artwork::File(path) => {
                let load = ImgResourceLoader::load(path.into(), cx);
                Box::pin(async move { load.await.ok() })
            }
            Artwork::Remote(url) => Box::pin(RemoteImage::load(url, cx)),
        };
        async move {
            let image = load.await.filter(|image| image.frame_count() > 0)?;
            thumbnail(image, source.pixels)
        }
    }
}

fn thumbnail(image: Arc<RenderImage>, pixels: u32) -> Option<Arc<RenderImage>> {
    let size = image.size(0);
    if image.frame_count() == 1 && size.width == size.height && u32::from(size.width) <= pixels {
        return Some(image);
    }
    let mut frames = Vec::new();
    let mut bytes = 0;
    for index in 0..image.frame_count() {
        let size = image.size(index);
        let (width, height) = (u32::from(size.width), u32::from(size.height));
        let input = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, image.as_bytes(index)?)?;
        let side = width.min(height);
        let target = side.min(pixels);
        // Large animations fall back to the still cover within the decoded cache limit.
        bytes += target as usize * target as usize * 4 + std::mem::size_of::<Frame>();
        if bytes > MAX_ALBUM_BYTES {
            frames.truncate(1);
            break;
        }
        // GPUI rounds the image before applying a rectangular cover crop.
        let crop = imageops::crop_imm(&input, (width - side) / 2, (height - side) / 2, side, side);
        let frame = imageops::thumbnail(&*crop, target, target);
        frames.push(Frame::from_parts(frame, 0, 0, image.delay(index)));
    }
    Some(Arc::new(RenderImage::new(frames)))
}

pub(super) fn remote_image(url: String) -> Img {
    img(move |window: &mut gpui::Window, cx: &mut App| {
        window.use_asset::<RemoteImage>(&url, cx).flatten().map(Ok)
    })
}

struct RemoteImage;
impl Asset for RemoteImage {
    type Source = String;
    type Output = Option<Arc<RenderImage>>;

    fn load(url: String, _: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        blocking::unblock(move || load_image(&url).ok().map(Arc::new))
    }
}

fn load_image(url: &str) -> anyhow::Result<RenderImage> {
    const MAX: u64 = 8 * 1024 * 1024;
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(MAX + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX, "Image download is too large.");
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let mut image = reader.decode()?.thumbnail(768, 768).into_rgba8();
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(RenderImage::new([image::Frame::new(image)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PlaybackPosition;

    fn seek_fixture() -> (PendingSeek, Player) {
        let seek = PendingSeek {
            player_id: "player".into(),
            track_id: "/track".into(),
            position: 123_500_000,
            previous_position: Some(6_000_000),
            requested_at: Instant::now(),
        };
        let player = Player {
            id: seek.player_id.clone(),
            app_id: "player".into(),
            name: "Player".into(),
            playback: Playback::Playing,
            track: None,
            position: None,
            rate: 1.,
            can_play: true,
            can_pause: true,
            can_previous: false,
            can_next: false,
            can_seek: true,
        };
        (seek, player)
    }

    #[test]
    fn pending_seek_advances_only_while_playing() {
        let (seek, mut player) = seek_fixture();
        let now = seek.requested_at + Duration::from_secs(1);
        assert_eq!(seek.displayed_position(&player, now), 124_500_000);
        player.playback = Playback::Paused;
        assert_eq!(seek.displayed_position(&player, now), 123_500_000);
    }

    #[test]
    fn seek_confirmation_uses_fresh_reports_and_accepts_rounding() {
        let (mut seek, mut player) = seek_fixture();
        let at = seek.requested_at + Duration::from_millis(900);
        player.position = Some(PlaybackPosition::new(
            seek.position,
            seek.requested_at - Duration::from_millis(1),
        ));
        assert!(!seek.confirmed(&player));
        player.position = Some(PlaybackPosition::new(7_000_000, at));
        assert!(!seek.confirmed(&player));
        player.position = Some(PlaybackPosition::new(122_000_000, at));
        assert!(seek.confirmed(&player));
        seek.previous_position = Some(122_000_000);
        assert!(!seek.confirmed(&player));
    }

    #[test]
    fn album_thumbnail_crops_to_the_center_at_the_display_size() {
        let cover = ImageBuffer::from_fn(12, 8, |x, _| {
            if (2..10).contains(&x) {
                Rgba([30, 60, 90, 255])
            } else {
                Rgba([0, 0, 0, 255])
            }
        });
        let image = Arc::new(RenderImage::new([Frame::new(cover)]));
        let resized = thumbnail(image, 4).unwrap();
        assert_eq!(u32::from(resized.size(0).width), 4);
        assert_eq!(u32::from(resized.size(0).height), 4);
        assert!(
            resized
                .as_bytes(0)
                .unwrap()
                .chunks_exact(4)
                .all(|pixel| pixel == [30, 60, 90, 255])
        );
    }
}
