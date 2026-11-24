use std::time::Duration;

use async_channel::Sender;
use futures_lite::future;
use gpui::{Context, IntoElement, SharedString, Styled, Task, div, prelude::*};
use jiff::{Span, Zoned, civil::Date};

use crate::platform::calendar::{self, Month};

use super::{Dashboard, Kind};
use crate::ui::theme::*;

#[derive(Default)]
pub(super) struct Calendar {
    month: Option<Date>,
    day: Option<Date>,
    feed: Option<Feed>,
}

struct Feed {
    requested: Date,
    snapshot: Option<Month>,
    requests: Sender<Date>,
    _task: Task<()>,
}

impl Dashboard {
    pub(super) fn sync_calendar(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.utilities.calendar;
        if !self.home.contains(Kind::Calendar) {
            state.feed = None;
            return;
        }
        let first = state
            .month
            .unwrap_or_else(|| Zoned::now().date().first_of_month());
        if let Some(feed) = &mut state.feed {
            if feed.requested != first {
                feed.requested = first;
                feed.snapshot = None;
                let _ = feed.requests.force_send(first);
            }
            return;
        }

        let (requests, receiver) = async_channel::bounded(1);
        let task = cx.spawn(async move |this, cx| {
            let mut month = first;
            loop {
                let snapshot = blocking::unblock(move || calendar::fetch(month)).await;
                if this
                    .update(cx, |dashboard, cx| {
                        if let Some(feed) = &mut dashboard.utilities.calendar.feed
                            && feed.requested == month
                        {
                            feed.snapshot = Some(snapshot);
                            if dashboard.home.shows(Kind::Calendar) {
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
                let timer = cx.background_executor().timer(Duration::from_secs(30));
                match future::or(receiver.recv(), async {
                    timer.await;
                    Ok(month)
                })
                .await
                {
                    Ok(next) => month = next,
                    Err(_) => break,
                }
            }
        });
        state.feed = Some(Feed {
            requested: first,
            snapshot: None,
            requests,
            _task: task,
        });
    }

    pub(in crate::ui) fn calendar(
        &self,
        theme: Theme,
        width: f32,
        height: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let state = &self.utilities.calendar;
        let today = Zoned::now().date();
        let first = state.month.unwrap_or(today.first_of_month());
        let selected = state
            .day
            .filter(|date| date.first_of_month() == first)
            .unwrap_or(if first == today.first_of_month() {
                today
            } else {
                first
            });
        let snapshot = state
            .feed
            .as_ref()
            .filter(|feed| feed.requested == first)
            .and_then(|feed| feed.snapshot.as_ref());
        let events = snapshot.map_or(&[][..], |month| month.events.as_slice());
        let compact = height < 180.;
        let side_by_side = compact || width >= 390.;
        let cell_height = if compact { 17. } else { 21. };
        let grid_width = (width * 0.47).min(208.);

        let mut nav = row().justify_between().gap(r(4.)).flex_none();
        for (id, text, delta) in [
            ("prev-month", "‹".to_owned(), -1),
            ("this-month", first.strftime("%B %Y").to_string(), 0),
            ("next-month", "›".to_owned(), 1),
        ] {
            let view = cx.entity();
            nav = nav.child(theme.bind(
                theme.button(id, None).h(r(23.)).px(r(7.)).child(text),
                move |_, cx| {
                    view.update(cx, |dashboard, cx| {
                        let state = &mut dashboard.utilities.calendar;
                        state.month = if delta == 0 {
                            None
                        } else {
                            first.checked_add(Span::new().months(delta)).ok()
                        };
                        state.day = None;
                        cx.notify();
                    })
                },
            ));
        }

        let cell = || {
            div()
                .flex_1()
                .min_w_0()
                .h(r(cell_height))
                .line_height(r(cell_height - 2.))
                .text_size(r(if compact { 10. } else { 12. }))
                .text_center()
                .whitespace_nowrap()
        };
        let start = first.weekday().to_monday_zero_offset();
        let weeks = (start + first.days_in_month() + 6) / 7;
        let weekdays = if side_by_side && grid_width < 120. {
            ["M", "T", "W", "T", "F", "S", "S"]
        } else {
            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
        };
        let mut grid = col()
            .id("calendar-days")
            .overflow_y_scroll()
            .when(side_by_side, |it| it.flex_none().w(r(grid_width)))
            .when(!side_by_side, |it| {
                it.flex_none().h(r(cell_height * f32::from(weeks + 1)))
            })
            .child(
                row()
                    .flex_none()
                    .text_color(theme.color(MUTED))
                    .children(weekdays.map(|name| cell().child(name))),
            );
        for week in 0..weeks {
            grid = grid.child(row().flex_none().children((0..7).map(|column| {
                let day = week * 7 + column - start + 1;
                let Ok(date) = first.with().day(day).build() else {
                    return cell().into_any_element();
                };
                let view = cx.entity();
                let marked = events.iter().any(|event| event.occurs_on(date));
                theme
                    .bind(
                        cell()
                            .id(("calendar-day", date.day() as u64))
                            .relative()
                            .rounded(r(4.))
                            .border_1()
                            .border_color(theme.color(STROKE).opacity(0.))
                            .hover(move |it| it.bg(theme.color(HOVER)))
                            .when(date == today, |it| it.border_color(theme.color(VIOLET)))
                            .when(date == selected, |it| {
                                it.bg(theme.color(VIOLET).opacity(0.22))
                            })
                            .child(day.to_string())
                            .when(marked, |it| {
                                it.child(
                                    div()
                                        .absolute()
                                        .bottom(r(0.))
                                        .left_0()
                                        .right_0()
                                        .flex()
                                        .justify_center()
                                        .child(theme.dot(VIOLET).size(r(3.))),
                                )
                            }),
                        move |_, cx| {
                            view.update(cx, |dashboard, cx| {
                                dashboard.utilities.calendar.day = Some(date);
                                cx.notify();
                            })
                        },
                    )
                    .into_any_element()
            })));
        }

        let body = if side_by_side { split() } else { col() };
        col().flex_1().gap(r(5.)).child(nav).child(
            body.flex_1()
                .min_h_0()
                .gap(r(if side_by_side { 8. } else { 5. }))
                .child(grid)
                .child(agenda(theme, selected, snapshot, compact)),
        )
    }
}

fn agenda(
    theme: Theme,
    selected: Date,
    snapshot: Option<&Month>,
    compact: bool,
) -> impl IntoElement {
    let mut events = col()
        .id(SharedString::from(format!("calendar-agenda-{selected}")))
        .flex_1()
        .overflow_y_scroll()
        .gap(r(4.));
    if let Some(snapshot) = snapshot {
        let mut entries = snapshot
            .events
            .iter()
            .filter(|event| event.occurs_on(selected))
            .peekable();
        if let Some(issue) = snapshot.issue {
            events = events.child(theme.quiet(issue).flex_none());
        } else if entries.peek().is_none() {
            events = events.child(theme.quiet("No events").flex_none());
        }
        events = events.children(entries.map(|event| {
            let time = if event.all_day {
                "All day".into()
            } else if event.start.date() < selected {
                "Continues".into()
            } else {
                event.start.strftime("%H:%M").to_string()
            };
            col()
                .flex_none()
                .border_l_1()
                .border_color(theme.color(VIOLET))
                .pl(r(5.))
                .when(compact, |it| it.line_height(r(14.)))
                .child(theme.quiet(time).text_size(r(11.)))
                .child(theme.label(event.title.clone()).text_size(r(12.)))
        }));
    } else {
        events = events.child(theme.quiet("Loading events…").flex_none());
    }
    col()
        .flex_1()
        .gap(r(3.))
        .child(
            theme
                .label(selected.strftime("%a, %b %-d").to_string())
                .flex_none()
                .truncate(),
        )
        .child(events)
}
