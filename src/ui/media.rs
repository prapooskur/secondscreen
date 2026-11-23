use std::{
    io::{Cursor, Read},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::model::{Artwork, MediaAction, Playback, Player, Track};
use gpui::{
    AnyElement, App, Asset, Context, Img, IntoElement, ObjectFit, RenderImage, Styled, div, img,
    prelude::*,
};

use super::{
    Dashboard,
    icons::{Icon, icon},
    slider::slider,
    theme::*,
};

const SEEK_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) struct PendingSeek {
    player_id: String,
    track_id: String,
    position: i64,
    pub(super) requested_at: Instant,
}

impl PendingSeek {
    fn matches(&self, player: &Player) -> bool {
        self.player_id == player.id
            && player
                .track
                .as_ref()
                .is_some_and(|track| self.track_id == track.id)
    }

    fn confirmed(&self, player: &Player, elapsed: Duration) -> bool {
        const TOLERANCE: i64 = 500_000;
        let advance = if player.playback == Playback::Playing {
            elapsed.as_micros() as i64
        } else {
            0
        };
        let limit = self.position.saturating_add(advance + TOLERANCE);
        player
            .position
            .is_some_and(|reported| (self.position - TOLERANCE..=limit).contains(&reported))
    }
}

impl Dashboard {
    pub(super) fn update_seek(&mut self) {
        if let Some(seek) = &self.pending_seek {
            let elapsed = seek.requested_at.elapsed();
            if elapsed >= SEEK_TIMEOUT
                || !self
                    .media
                    .players
                    .iter()
                    .any(|player| seek.matches(player) && !seek.confirmed(player, elapsed))
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
        let action = MediaAction::Seek {
            track: track_id.to_owned(),
            position,
            requested_at,
        };
        if self
            .controls
            .media
            .send((player_id.to_owned(), action))
            .is_ok()
        {
            self.pending_seek = Some(PendingSeek {
                player_id: player_id.to_owned(),
                track_id: track_id.to_owned(),
                position,
                requested_at,
            });
            self.error = None;
        } else {
            self.pending_seek = None;
            self.error = Some((requested_at, "The media worker stopped.".into()));
        }
        cx.notify();
    }

    pub(super) fn media_progress(&self, player: &Player, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let track = player.track.as_ref();
        let length = track.and_then(|track| track.duration);
        let slider_id = seek_id(player, track);
        let current = self
            .pending_seek
            .as_ref()
            .filter(|seek| seek.matches(player))
            .map(|seek| seek.position)
            .or(player.position);
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
    let art = artwork.map(|artwork| {
        match artwork {
            Artwork::File(path) => img(path),
            Artwork::Remote(url) => remote_image(url),
        }
        .absolute()
        .inset_0()
        .size_full()
        .object_fit(ObjectFit::Cover)
    });

    row()
        .size(r(side))
        .max_h_full()
        .flex_none()
        .relative()
        .overflow_hidden()
        .justify_center()
        .rounded(r(10.))
        .border_1()
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
