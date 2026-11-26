use std::{
    cell::Cell,
    collections::VecDeque,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    Context, Div, FontWeight, ListAlignment, ListOffset, ListState, Pixels, SharedString, Task,
    Window, div, ease_in_out, list, point, prelude::*, px,
};

use crate::{
    lyrics::{LyricsQuery, SyncedLyrics},
    model::{Playback, Player},
    ui::{Dashboard, theme::*},
};

const CACHE_SIZE: usize = 32;
const RETRY_DELAY: Duration = Duration::from_secs(60);
const LYRIC_SCROLL_DURATION: Duration = Duration::from_millis(240);

#[derive(Default)]
pub(in crate::ui) struct MediaLyrics {
    pub(in crate::ui) enabled: bool,
    cache: VecDeque<(LyricsQuery, LookupResult)>,
    lookup: Option<Task<()>>,
    timer: Option<Task<()>>,
    view: Option<LyricsView>,
}

enum LookupResult {
    Complete(Option<Arc<SyncedLyrics>>),
    RetryAt(Instant),
}

struct LyricsView {
    lyrics: Arc<SyncedLyrics>,
    list: ListState,
    cue: usize,
    target: ListOffset,
    scroll: Rc<Cell<Option<LyricScroll>>>,
}

#[derive(Clone, Copy)]
struct LyricScroll {
    from: ListOffset,
    started_at: Instant,
}

impl LyricsView {
    fn new(lyrics: Arc<SyncedLyrics>, cue: usize, target: ListOffset) -> Self {
        let list = ListState::new(lyrics.line_count(), ListAlignment::Top, px(120.));
        let scroll = Rc::new(Cell::new(None));
        let manual_scroll = scroll.clone();
        list.set_scroll_handler(move |_, _, _| manual_scroll.set(None));
        list.scroll_to(target);
        Self {
            lyrics,
            list,
            cue,
            target,
            scroll,
        }
    }

    fn offset_at(&self, anchor: ListOffset) -> Option<Pixels> {
        self.list.scroll_to(anchor);
        self.list.bounds_for_item(anchor.item_ix)?;
        let offset = self.list.scroll_px_offset_for_scrollbar().y;
        Some(offset.max(-self.list.max_offset_for_scrollbar().y))
    }

    fn animate_scroll(&self, window: &Window, reduce_motion: bool) {
        let Some(scroll) = self.scroll.get() else {
            return;
        };
        let progress = scroll
            .started_at
            .elapsed()
            .div_duration_f32(LYRIC_SCROLL_DURATION);
        if !reduce_motion
            && progress < 1.
            && let Some((from, to)) = self.offset_at(scroll.from).zip(self.offset_at(self.target))
            && (to - from).abs() > px(0.5)
        {
            let offset = from + (to - from) * ease_in_out(progress);
            self.list.set_offset_from_scrollbar(point(px(0.), offset));
            window.request_animation_frame();
        } else {
            // A seek can land beyond the rows the list has measured.
            self.list.scroll_to(self.target);
            self.scroll.set(None);
        }
    }
}

impl MediaLyrics {
    fn cached(&self, query: &LyricsQuery) -> Option<&LookupResult> {
        self.cache
            .iter()
            .find(|(key, _)| key == query)
            .map(|(_, result)| result)
    }

    fn store(&mut self, query: LyricsQuery, result: LookupResult) {
        self.cache.retain(|(key, _)| key != &query);
        if self.cache.len() >= CACHE_SIZE {
            self.cache.pop_front();
        }
        self.cache.push_back((query, result));
    }

    fn needs_lookup(&self, query: &LyricsQuery) -> bool {
        match self.cached(query) {
            Some(LookupResult::Complete(_)) => false,
            Some(LookupResult::RetryAt(at)) => Instant::now() >= *at,
            None => true,
        }
    }

    fn follow(&mut self, lyrics: Arc<SyncedLyrics>, cue: usize, height: f32) {
        let target = ListOffset {
            item_ix: cue.saturating_sub(if height >= 152. { 2 } else { 1 }),
            offset_in_item: px(0.),
        };
        let Some(view) = self.view.as_mut().filter(|view| view.lyrics == lyrics) else {
            self.view = Some(LyricsView::new(lyrics, cue, target));
            return;
        };
        if view.cue != cue || view.target != target {
            let from = view.list.logical_scroll_top();
            view.scroll.set((from != target).then(|| LyricScroll {
                from,
                started_at: Instant::now(),
            }));
        }
        view.lyrics = lyrics;
        view.cue = cue;
        view.target = target;
    }
}

impl Dashboard {
    pub(in crate::ui) fn sync_media_lyrics(&mut self, cx: &mut Context<Self>) {
        let height = self.home.media_lyrics_height();
        if height.is_some() && self.media_lyrics.lookup.is_none() {
            self.start_lyrics_lookup(cx);
        }
        let cue = self.retime_media_lyrics(cx);
        if let Some((height, cue)) = height.zip(cue)
            && let Some(lyrics) = self
                .current_player()
                .and_then(|player| self.lyrics_for(player))
                .cloned()
        {
            self.media_lyrics.follow(lyrics, cue, height);
        } else {
            self.media_lyrics.view = None;
        }
    }

    pub(in crate::ui) fn animate_lyrics_scroll(&self, window: &Window, cx: &Context<Self>) {
        if let Some(view) = &self.media_lyrics.view {
            view.animate_scroll(window, cx.reduce_motion());
        }
    }

    fn start_lyrics_lookup(&mut self, cx: &mut Context<Self>) {
        let query = self
            .current_player()
            .and_then(|player| player.track.as_ref())
            .filter(|track| track.lyrics.is_none())
            .and_then(LyricsQuery::for_track)
            .filter(|query| self.media_lyrics.needs_lookup(query));
        let Some(query) = query else { return };

        // One request at a time. A response stays with its own query when the track changes.
        self.media_lyrics.lookup = Some(cx.spawn(async move |this, cx| {
            let lookup = query.clone();
            let result = match blocking::unblock(move || lookup.fetch()).await {
                Ok(lyrics) => LookupResult::Complete(lyrics.map(Arc::new)),
                Err(error) => {
                    log::debug!("Synced lyrics lookup failed: {}", error.root_cause());
                    LookupResult::RetryAt(Instant::now() + RETRY_DELAY)
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.media_lyrics.lookup = None;
                this.media_lyrics.store(query, result);
                if this.home.media_lyrics_height().is_some() {
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn lyrics_for<'a>(&'a self, player: &'a Player) -> Option<&'a Arc<SyncedLyrics>> {
        let track = player.track.as_ref()?;
        if let Some(lyrics) = &track.lyrics {
            return Some(lyrics);
        }
        let query = LyricsQuery::for_track(track)?;
        match self.media_lyrics.cached(&query)? {
            LookupResult::Complete(lyrics) => lyrics.as_ref(),
            LookupResult::RetryAt(_) => None,
        }
    }

    pub(in crate::ui) fn media_lyric_changed(&mut self, cx: &mut Context<Self>) -> bool {
        self.retime_media_lyrics(cx) != self.media_lyrics.view.as_ref().map(|view| view.cue)
    }

    fn retime_media_lyrics(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        let (cue, deadline) = self.lyric_timing().unzip();
        self.media_lyrics.timer = deadline.flatten().map(|deadline| {
            let timer = cx
                .background_executor()
                .timer(deadline.saturating_duration_since(Instant::now()));
            cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
        });
        cue
    }

    fn lyric_timing(&self) -> Option<(usize, Option<Instant>)> {
        if !self.media_lyrics.enabled || self.home.media_lyrics_height().is_none() {
            return None;
        }
        let now = Instant::now();
        let player = self.current_player()?;
        let lyrics = self.lyrics_for(player)?;
        let position = self.displayed_media_position(player, now)?;
        let cue = lyrics.passed(position);
        let deadline = (|| {
            if player.playback != Playback::Playing {
                return None;
            }
            let boundary = lyrics.next_boundary(cue, player.rate)?;
            let distance = boundary.abs_diff(position) as f64;
            let delay =
                Duration::try_from_secs_f64(distance / player.rate.abs() / 1_000_000. + 0.001)
                    .ok()?;
            let deadline = now.checked_add(delay)?;
            let future = self.displayed_media_position(player, deadline)?;
            // Wait for a fresh position report if the projected clock stalls before the boundary.
            (lyrics.passed(future) != cue).then_some(deadline)
        })();
        Some((cue, deadline))
    }

    pub(super) fn media_lyrics_panel(&self, theme: Theme, height: f32) -> Div {
        let Some(view) = &self.media_lyrics.view else {
            return theme
                .quiet("Playback position unavailable")
                .line_height(r(14.))
                .line_clamp(2)
                .min_w_0();
        };
        lyrics_panel(theme, view, height)
    }
}

fn lyrics_panel(theme: Theme, view: &LyricsView, height: f32) -> Div {
    let lyrics = view.lyrics.clone();
    let cue = view.cue;
    let compact = height < 92.;
    let (text_size, line_height, line_limit, spacing) = if compact {
        (13., 16., 2, 2.)
    } else {
        (17., 22., 3, 6.)
    };
    let lines = list(view.list.clone(), move |index, _, _| {
        let Some(text) = lyrics.line(index) else {
            return div().h(r(spacing)).into_any_element();
        };
        let color = if index + 1 < cue {
            theme.color(MUTED).opacity(0.8)
        } else {
            theme.color(MUTED)
        };
        div()
            .py(r(spacing))
            .text_size(r(text_size))
            .line_height(r(line_height))
            .line_clamp(line_limit)
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .when(index + 1 == cue, |it| {
                let active_size = text_size + 4.;
                it.text_size(r(active_size))
                    .line_height(r(line_height.max(active_size)))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.color(INK))
            })
            .child(SharedString::from(text))
            .into_any_element()
    });
    col()
        .h(r(height))
        .overflow_hidden()
        .child(
            lines
                .flex_1()
                .w_full()
                .px(r(if compact { 6. } else { 10. })),
        )
}
