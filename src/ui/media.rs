use std::{
    io::{Cursor, Read},
    sync::Arc,
    time::Duration,
};

use crate::model::{Artwork, MediaAction, Playback, Player};
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

impl Dashboard {
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
        let playing = player.playback == Playback::Playing;
        let (status, color) = match player.playback {
            Playback::Playing => ("Playing", TEAL),
            Playback::Paused => ("Paused", AMBER),
            Playback::Stopped => ("Stopped", MUTED),
        };
        let direction = |id, symbol, can, which| {
            let control = button(id, None)
                .size(r(39.))
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
            .size(r(43.))
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

        let length = track.and_then(|track| track.duration);
        let progress = player
            .position
            .zip(length)
            .map_or(0., |(at, total)| (at as f32 / total as f32).clamp(0., 1.));
        let can_seek = player.can_seek
            && length.is_some()
            && track.is_some_and(|track| track.id.starts_with('/'));
        let position = self
            .gesture
            .borrow()
            .as_ref()
            .filter(|(id, _)| id == "seek")
            .and_then(|(_, ratio)| length.map(|length| (*ratio * length as f32) as i64))
            .or(player.position);
        let seek = {
            let id = player.id.clone();
            let track_id = track.map(|track| track.id.clone()).unwrap_or_default();
            let view = cx.entity();
            slider(
                "seek".to_owned(),
                progress,
                BLUE,
                can_seek,
                self.gesture.clone(),
                0.02,
                move |fraction, cx| {
                    if let Some(length) = length {
                        let position =
                            ((fraction * length as f32) as i64).clamp(0, length.saturating_sub(1));
                        view.read(cx).send_media(
                            id.clone(),
                            MediaAction::Seek {
                                track: track_id.clone(),
                                position,
                            },
                        );
                    }
                },
            )
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
                    .child(album_art(track.and_then(|track| track.artwork.clone())))
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
            .child(
                row()
                    .gap(r(10.))
                    .flex_none()
                    .child(
                        mono(position.map(timecode).unwrap_or_else(|| "—:—".to_owned()))
                            .text_size(r(11.))
                            .w(r(35.))
                            .text_color(c(MUTED)),
                    )
                    .child(seek)
                    .child(
                        mono(length.map(timecode).unwrap_or_else(|| "—:—".to_owned()))
                            .text_size(r(11.))
                            .w(r(35.))
                            .text_right()
                            .text_color(c(MUTED)),
                    ),
            )
            .child(
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
                    .child(direction("next", Icon::Next, player.can_next, true)),
            )
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

fn timecode(micros: i64) -> String {
    let seconds = micros.max(0) / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn album_art(artwork: Option<Artwork>) -> impl IntoElement {
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
        .size(r(123.))
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
                .size(r(83.))
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(c(BLUE).opacity(0.23))
                .child(
                    row()
                        .size(r(61.))
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(c(BLUE).opacity(0.29))
                        .child(icon(Icon::Music, c(BLUE), 27.)),
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
