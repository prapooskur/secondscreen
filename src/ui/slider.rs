use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, HitboxBehavior, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Styled, canvas, fill, point, prelude::*,
    px, size,
};
use std::{cell::RefCell, rc::Rc};

use super::theme::*;

pub type Gesture = Rc<RefCell<Option<(String, f32)>>>;

pub fn slider(
    id: String,
    value: f32,
    accent: u32,
    enabled: bool,
    gesture: Gesture,
    step: f32,
    change: impl Fn(f32, &mut App) + 'static,
) -> impl IntoElement {
    let change = Rc::new(change);
    let keyboard = change.clone();
    let track_id = id.clone();
    let track = canvas(
        move |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            let radius = px(5.);
            let left = bounds.left() + radius;
            let width = (bounds.size.width - radius * 2.).max(px(1.));
            let center = bounds.center().y;
            let current = gesture
                .borrow()
                .as_ref()
                .filter(|(id, _)| id == &track_id)
                .map_or(value, |(_, value)| *value)
                .clamp(0., 1.);
            let line = |length: Pixels, color| {
                fill(
                    Bounds::new(point(left, center - px(2.)), size(length, px(4.))),
                    color,
                )
                .corner_radii(px(2.))
            };
            window.paint_quad(line(width, c(STROKE)));
            if current > 0. {
                window.paint_quad(line(
                    width * current,
                    c(accent).opacity(if enabled { 0.85 } else { 0.4 }),
                ));
            }
            if enabled {
                let thumb = Bounds::new(
                    point(left + width * current - radius, center - radius),
                    size(radius * 2., radius * 2.),
                );
                window.paint_quad(fill(thumb, c(accent)).corner_radii(radius));
                window.set_cursor_style(CursorStyle::PointingHand, &hitbox);

                let fraction = move |x: Pixels| ((x - left) / width).clamp(0., 1.);
                let start = gesture.clone();
                let start_id = track_id.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        *start.borrow_mut() = Some((start_id.clone(), fraction(event.position.x)));
                        cx.stop_propagation();
                        window.refresh();
                    }
                });
                let progress = gesture.clone();
                let progress_id = track_id.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                    if phase == DispatchPhase::Bubble
                        && event.pressed_button == Some(MouseButton::Left)
                        && let Some((_, value)) = progress
                            .borrow_mut()
                            .as_mut()
                            .filter(|(id, _)| id == &progress_id)
                    {
                        *value = fraction(event.position.x);
                        window.refresh();
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && gesture
                            .borrow()
                            .as_ref()
                            .is_some_and(|(id, _)| id == &track_id)
                    {
                        gesture.borrow_mut().take();
                        change(fraction(event.position.x), cx);
                        window.refresh();
                    }
                });
            }
        },
    )
    .w_full()
    .h(r(25.));

    row()
        .id(gpui::SharedString::from(id))
        .flex_1()
        .border_1()
        .border_color(c(PANEL))
        .rounded(r(5.))
        .when(enabled, |it| {
            it.tab_stop(true)
                .focus(|it| it.border_color(c(TEAL)))
                .on_key_down(move |event, _, cx| {
                    let next = match event.keystroke.key.as_str() {
                        "left" | "down" => value - step,
                        "right" | "up" => value + step,
                        "home" => 0.,
                        "end" => 1.,
                        _ => return,
                    };
                    cx.stop_propagation();
                    keyboard(next.clamp(0., 1.), cx);
                })
        })
        .child(track)
}
