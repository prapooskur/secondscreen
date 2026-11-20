use std::{
    io::{Cursor, Read},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::model::{Artwork, MediaAction, Playback, Player, Track};
use gpui::{
    App, Asset, Context, FontWeight, IntoElement, ObjectFit, RenderImage, Styled, div, img,
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

    pub(super) fn media_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.current_player() {
            Some(player) => split()
                .flex_1()
                .min_h_0()
                .gap(r(12.))
                .child(self.now_playing(player, cx))
                .child(self.player_list(player, cx))
                .into_any_element(),
            None => panel()
                .flex_1()
                .child(empty(
                    Icon::Music,
                    BLUE,
                    if self.media.issue.is_some() {
                        "Media service unavailable"
                    } else {
                        "No media players"
                    },
                    self.media.issue.clone().unwrap_or_else(|| {
                        "Play something in a desktop app or browser that supports MPRIS.".into()
                    }),
                ))
                .into_any_element(),
        };

        col()
            .flex_1()
            .gap(r(12.))
            .child(section_header("Media playback", None))
            .child(content)
    }

    fn now_playing(&self, player: &Player, cx: &Context<Self>) -> impl IntoElement {
        let track = player.track.as_ref();
        let (status, color) = match player.playback {
            Playback::Playing => ("Playing", TEAL),
            Playback::Paused => ("Paused", AMBER),
            Playback::Stopped => ("Stopped", MUTED),
        };
        panel()
            .flex_1()
            .p(r(14.))
            .gap(r(10.))
            .overflow_hidden()
            .child(
                row()
                    .flex_1()
                    .min_h_0()
                    .gap(r(17.))
                    .child(album_art(
                        track.and_then(|track| track.artwork.clone()),
                        123.,
                    ))
                    .child(
                        col()
                            .flex_1()
                            .gap(r(7.))
                            .child(
                                row()
                                    .gap(r(8.))
                                    .child(chip(status, color))
                                    .child(quiet(player.name.clone()).truncate()),
                            )
                            .child(
                                div()
                                    .text_size(r(23.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(
                                        track
                                            .map(|track| track.title.clone())
                                            .unwrap_or_else(|| "No track selected".into()),
                                    ),
                            )
                            .when_some(track, |it, track| {
                                it.when(!track.artist.is_empty(), |it| {
                                    it.child(label(track.artist.clone()).truncate())
                                })
                                .when(!track.album.is_empty(), |it| {
                                    it.child(quiet(track.album.clone()).truncate())
                                })
                            }),
                    ),
            )
            .child(self.media_progress(player, cx))
            .child(self.media_transport(player, false, cx))
    }

    pub(super) fn media_progress(&self, player: &Player, cx: &Context<Self>) -> impl IntoElement {
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
                    .text_color(c(MUTED)),
            )
            .child(seek)
            .child(
                mono(length.map(timecode).unwrap_or_else(|| "—:—".to_owned()))
                    .text_size(r(11.))
                    .min_w(r(35.))
                    .flex_none()
                    .whitespace_nowrap()
                    .text_right()
                    .text_color(c(MUTED)),
            )
    }

    pub(super) fn media_transport(
        &self,
        player: &Player,
        compact: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let playing = player.playback == Playback::Playing;
        let direction = |id, symbol, can, which| {
            let control = button(id, None)
                .size(r(if compact { 33. } else { 39. }))
                .px_0()
                .child(icon(symbol, c(TEXT), 16.));
            if can {
                let view = cx.entity();
                let id = player.id.clone();
                bind(control, move |_, cx| {
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
        let toggle = button("play-pause", Some(TEAL))
            .size(r(if compact { 37. } else { 43. }))
            .px_0()
            .child(icon(
                if playing { Icon::Pause } else { Icon::Play },
                c(TEAL),
                20.,
            ));
        let toggle = if player.can_toggle_playback() {
            let view = cx.entity();
            let id = player.id.clone();
            bind(toggle, move |_, cx| {
                view.read(cx).send_media(id.clone(), MediaAction::Toggle)
            })
        } else {
            disabled(toggle)
        };

        row()
            .justify_center()
            .gap(r(8.))
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

    fn player_list(&self, active: &Player, cx: &Context<Self>) -> impl IntoElement {
        panel()
            .w(gpui::relative(0.32))
            .overflow_hidden()
            .child(
                row()
                    .h(r(50.))
                    .flex_none()
                    .px(r(12.))
                    .gap(r(8.))
                    .border_b_1()
                    .border_color(c(STROKE))
                    .child(icon_tile(Icon::Music, BLUE, 28.))
                    .child(label("Players").text_color(c(INK))),
            )
            .child(
                col()
                    .id("players")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(r(10.))
                    .gap(r(6.))
                    .children(self.media.players.iter().map(|player| {
                        let selected = player.id == active.id;
                        let id = player.id.clone();
                        let view = cx.entity();
                        bind(
                            button(
                                gpui::SharedString::from(player.id.clone()),
                                selected.then_some(BLUE),
                            )
                            .justify_start()
                            .child(icon(
                                if selected { Icon::Check } else { Icon::Music },
                                c(if selected { BLUE } else { MUTED }),
                                15.,
                            ))
                            .child(div().flex_1().truncate().child(player.name.clone()))
                            .when(player.playback == Playback::Playing, |it| {
                                it.child(dot(TEAL))
                            }),
                            move |_, cx| {
                                view.update(cx, |this, cx| {
                                    this.player = Some(id.clone());
                                    this.gesture.borrow_mut().take();
                                    cx.notify();
                                })
                            },
                        )
                    })),
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

pub(super) fn album_art(artwork: Option<Artwork>, side: f32) -> impl IntoElement {
    let art = artwork.map(|artwork| {
        match artwork {
            Artwork::File(path) => img(path),
            Artwork::Remote(url) => img(move |window: &mut gpui::Window, cx: &mut App| {
                window.use_asset::<RemoteArt>(&url, cx).flatten().map(Ok)
            }),
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
        .border_color(c(STROKE))
        .bg(c(0x172842))
        .child(
            row()
                .size(r(side * 0.675))
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(c(BLUE).opacity(0.23))
                .child(
                    row()
                        .size(r(side * 0.496))
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(c(BLUE).opacity(0.29))
                        .child(icon(Icon::Music, c(BLUE), side * 0.22)),
                ),
        )
        .children(art)
}

struct RemoteArt;
impl Asset for RemoteArt {
    type Source = String;
    type Output = Option<Arc<RenderImage>>;

    fn load(url: String, _: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        blocking::unblock(move || load_art(&url).ok().map(Arc::new))
    }
}

fn load_art(url: &str) -> anyhow::Result<RenderImage> {
    const MAX: u64 = 8 * 1024 * 1024;
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(MAX + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX, "Artwork download is too large.");
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
