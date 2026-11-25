use std::cell::Cell;

use gpui::{
    App, Context, InputEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollDelta, ScrollEvent, ScrollSource, Task, TouchPhase, Window, point, px,
};

use crate::platform::touch::{Contact, Event, TouchSource};

use super::Dashboard;

const TAP_SLOP: Pixels = px(8.);

#[derive(Clone, Copy)]
pub(super) enum DragTarget {
    Scroll,
    Horizontal,
    Free,
}

thread_local! {
    // A move lets GPUI hit-test the contact with the current clipping and overlays.
    static DRAG_PROBE: Cell<Option<DragTarget>> = const { Cell::new(None) };
}

pub(super) fn offer_drag(target: DragTarget) {
    DRAG_PROBE.with(|probe| {
        if matches!(probe.get(), Some(DragTarget::Scroll)) {
            probe.set(Some(target));
        }
    });
}

pub(super) struct TouchInput {
    _source: TouchSource,
    _dispatch: Task<()>,
}

impl TouchInput {
    pub(super) fn start(window: &Window, cx: &Context<Dashboard>) -> Option<Self> {
        let (source, events) = match TouchSource::start(window) {
            Ok(input) => input,
            Err(error) => {
                log::debug!("Could not start Wayland touch input: {error:#}");
                return None;
            }
        };
        let dispatch = cx.spawn_in(window, async move |_, cx| {
            let mut gestures = Gestures::default();
            while let Ok(event) = events.recv().await {
                if cx
                    .update(|window, cx| gestures.handle(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Some(Self {
            _source: source,
            _dispatch: dispatch,
        })
    }
}

#[derive(Clone, Copy)]
enum Gesture {
    Tap(DragTarget),
    Drag,
    Pan { horizontal: bool },
}

struct ActiveContact {
    id: Contact,
    origin: Point<Pixels>,
    last: Point<Pixels>,
    gesture: Gesture,
}

#[derive(Default)]
struct Gestures {
    active: Option<ActiveContact>,
}

impl Gestures {
    fn handle(&mut self, event: Event, window: &mut Window, cx: &mut App) {
        match event {
            Event::Down(id, x, y) if self.active.is_none() => {
                let origin = point(px(x), px(y));
                DRAG_PROBE.with(|probe| probe.set(Some(DragTarget::Scroll)));
                move_pointer(origin, false, window, cx);
                let target = DRAG_PROBE.with(Cell::take).unwrap_or(DragTarget::Scroll);
                self.active = Some(ActiveContact {
                    id,
                    origin,
                    last: origin,
                    gesture: Gesture::Tap(target),
                });
            }
            Event::Motion(id, x, y) => {
                if let Some(active) = self.active.as_mut().filter(|active| active.id == id) {
                    active.move_to(point(px(x), px(y)), window, cx);
                }
            }
            Event::Up(id) if self.active.as_ref().is_some_and(|active| active.id == id) => {
                if let Some(active) = self.active.take() {
                    active.finish(false, window, cx);
                }
            }
            Event::Cancel(seat)
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.id.seat == seat) =>
            {
                if let Some(active) = self.active.take() {
                    active.finish(true, window, cx);
                }
            }
            _ => {}
        }
    }
}

impl ActiveContact {
    fn move_to(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        let mut phase = TouchPhase::Moved;
        if let Gesture::Tap(target) = self.gesture {
            let total = position - self.origin;
            if total.x.abs().max(total.y.abs()) < TAP_SLOP {
                return;
            }
            let horizontal = total.x.abs() >= total.y.abs();
            self.gesture = match target {
                DragTarget::Free => Gesture::Drag,
                DragTarget::Horizontal if horizontal => Gesture::Drag,
                _ => Gesture::Pan { horizontal },
            };
            if matches!(self.gesture, Gesture::Drag) {
                press(self.origin, window, cx);
            }
            phase = TouchPhase::Started;
        }
        match self.gesture {
            Gesture::Drag => move_pointer(position, true, window, cx),
            Gesture::Pan { horizontal } => {
                let delta = position - self.last;
                let delta = if horizontal {
                    point(delta.x, px(0.))
                } else {
                    point(px(0.), delta.y)
                };
                if delta != Point::default() {
                    scroll(self.origin, delta, phase, window, cx);
                }
            }
            Gesture::Tap(_) => {}
        }
        self.last = position;
    }

    fn finish(self, cancelled: bool, window: &mut Window, cx: &mut App) {
        let outside = point(px(-1.), px(-1.));
        match self.gesture {
            Gesture::Tap(_) if !cancelled => {
                press(self.origin, window, cx);
                release(self.origin, window, cx);
            }
            Gesture::Drag if cancelled => {
                move_pointer(outside, false, window, cx);
                release(outside, window, cx);
                return;
            }
            Gesture::Drag => release(self.last, window, cx),
            Gesture::Pan { .. } => {
                scroll(self.origin, Point::default(), TouchPhase::Ended, window, cx)
            }
            Gesture::Tap(_) => {}
        }
        move_pointer(outside, false, window, cx);
    }
}

fn press(position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        MouseDownEvent {
            position,
            modifiers: window.modifiers(),
            click_count: 1,
            ..Default::default()
        }
        .to_platform_input(),
        cx,
    );
}

fn release(position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        MouseUpEvent {
            position,
            modifiers: window.modifiers(),
            click_count: 1,
            ..Default::default()
        }
        .to_platform_input(),
        cx,
    );
}

fn move_pointer(position: Point<Pixels>, dragging: bool, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        MouseMoveEvent {
            position,
            pressed_button: dragging.then_some(MouseButton::Left),
            modifiers: window.modifiers(),
        }
        .to_platform_input(),
        cx,
    );
}

fn scroll(
    position: Point<Pixels>,
    delta: Point<Pixels>,
    touch_phase: TouchPhase,
    window: &mut Window,
    cx: &mut App,
) {
    window.dispatch_event(
        ScrollEvent {
            position,
            delta: ScrollDelta::Pixels(delta),
            touch_phase,
            modifiers: window.modifiers(),
            source: ScrollSource::Touch,
        }
        .to_platform_input(),
        cx,
    );
}
