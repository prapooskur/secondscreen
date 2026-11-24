use std::{
    io::{Cursor, Read},
    sync::Arc,
    time::{Duration, Instant},
};

use image::{Frame, ImageBuffer, Rgba, imageops};

use crate::model::{Artwork, MediaAction, MediaState, Playback, Player, Track};
use gpui::{
    AnyElement, App, Asset, Context, Global, Img, ImgResourceLoader, IntoElement, ObjectFit,
    RenderImage, Styled, Window, div, img, prelude::*,
};

use super::{
    Dashboard,
    icons::{Icon, icon},
    slider::slider,
    theme::*,
};

const SEEK_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_ALBUM_SIDE: u32 = 768;
const MAX_ALBUM_BYTES: usize = 8 * 1024 * 1024;

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
                player
                    .track
                    .as_ref()
                    .is_some_and(|track| *id == seek_id(player, Some(track)))
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

    pub(super) fn media_progress(&self, player: &Player, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let track = player.track.as_ref();
        let length = track.and_then(|track| track.duration);
        let slider_id = seek_id(player, track);
        let now = Instant::now();
        let current = self
            .pending_seek
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
            });
        let progress = current
            .zip(length)
            .map_or(0., |(at, total)| (at as f32 / total as f32).clamp(0., 1.));
        let can_seek = player.can_seek
            && length.is_some()
            && track.is_some_and(|track| track.id.starts_with('/'));
        let position = self
            .gesture
            .borrow()
            .as_ref()
            .filter(|(id, _)| id == &slider_id)
            .and_then(|(_, ratio)| length.map(|length| seek_position(*ratio, length)))
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
                BLUE,
                can_seek,
                self.gesture.clone(),
                0.02,
                move |fraction, cx| {
                    if let Some(length) = length {
                        view.update(cx, |this, cx| {
                            this.seek_media(&id, &track_id, seek_position(fraction, length), cx);
                        });
                    }
                },
            )
        };

        row()
            .gap(r(10.))
            .flex_none()
            .child(
                mono(position.map(timecode).unwrap_or_else(|| "—:—".to_owned()))
                    .text_size(r(11.))
                    .min_w(r(35.))
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(theme.color(MUTED)),
            )
            .child(seek)
            .child(
                mono(length.map(timecode).unwrap_or_else(|| "—:—".to_owned()))
                    .text_size(r(11.))
                    .min_w(r(35.))
                    .flex_none()
                    .whitespace_nowrap()
                    .text_right()
                    .text_color(theme.color(MUTED)),
            )
    }

    pub(super) fn media_transport(
        &self,
        player: &Player,
        compact: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let playing = player.playback == Playback::Playing;
        let direction = |id, symbol, can, which| {
            let control = theme
                .button(id, None)
                .size(r(if compact { 26. } else { 33. }))
                .px_0()
                .child(icon(
                    symbol,
                    theme.color(TEXT),
                    if compact { 13. } else { 16. },
                ));
            if can {
                let view = cx.entity();
                let id = player.id.clone();
                theme.bind(control, move |_, cx| {
                    view.read(cx).send_media(
                        id.clone(),
                        if which {
                            MediaAction::Next
                        } else {
                            MediaAction::Previous
                        },
                    );
                })
            } else {
                disabled(control)
            }
        };
        let toggle = theme
            .button("play-pause", Some(TEAL))
            .size(r(if compact { 30. } else { 37. }))
            .px_0()
            .child(icon(
                if playing { Icon::Pause } else { Icon::Play },
                theme.color(TEAL),
                if compact { 16. } else { 20. },
            ));
        let toggle = if player.can_toggle_playback() {
            let view = cx.entity();
            let id = player.id.clone();
            theme.bind(toggle, move |_, cx| {
                view.read(cx).send_media(id.clone(), MediaAction::Toggle)
            })
        } else {
            disabled(toggle)
        };

        row()
            .justify_center()
            .gap(r(if compact { 4. } else { 8. }))
            .flex_none()
            .child(direction(
                "previous",
                Icon::Previous,
                player.can_previous,
                false,
            ))
            .child(toggle)
            .child(direction("next", Icon::Next, player.can_next, true))
    }

    pub(super) fn player_control(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        let active = self.current_player()?;
        let mut players = self.visible_players();
        players.find(|player| player.id == active.id)?;
        let next = players.next().or_else(|| self.visible_players().next())?;
        if next.id == active.id {
            return Some(
                theme
                    .quiet(active.name.clone())
                    .max_w(gpui::relative(0.48))
                    .truncate()
                    .into_any_element(),
            );
        }
        let view = cx.entity();
        let id = next.id.clone();
        Some(
            theme
                .bind(
                    theme
                        .button("switch-player", None)
                        .h(r(27.))
                        .max_w(gpui::relative(0.48))
                        .px(r(6.))
                        .child(div().truncate().child(active.name.clone()))
                        .child(icon(Icon::Chevron, theme.color(TEXT), 12.))
                        .tooltip(|_, cx| {
                            cx.new(|_| super::Tooltip("Switch media player".into()))
                                .into()
                        }),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.player = Some(id.clone());
                            this.gesture.borrow_mut().take();
                            cx.notify();
                        })
                    },
                )
                .into_any_element(),
        )
    }
}

fn seek_id(player: &Player, track: Option<&Track>) -> String {
    let track_id = track.map_or("", |track| track.id.as_str());
    format!("seek:{}:{track_id}", player.id)
}

fn seek_position(fraction: f32, duration: i64) -> i64 {
    // An MPRIS SetPosition at or beyond the duration is ignored by the player.
    ((f64::from(fraction) * duration as f64).round() as i64).clamp(0, duration - 1)
}

fn timecode(micros: i64) -> String {
    let seconds = micros.max(0) / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub(super) fn album_art(theme: Theme, artwork: Option<Artwork>, side: f32) -> impl IntoElement {
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

    let mut thumbnail = row()
        .h_full()
        .max_h(r(side))
        .max_w(r(side))
        .flex_none()
        .relative()
        .overflow_hidden()
        .justify_center()
        .rounded(r(radius))
        .border(r(1.))
        .border_color(theme.color(STROKE))
        .bg(theme.color(ARTWORK_BACK));
    thumbnail.style().aspect_ratio = Some(1.);
    thumbnail
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
