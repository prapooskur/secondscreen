use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, HitboxBehavior, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Styled, canvas, fill, point, prelude::*,
    px, size,
};
use std::{cell::RefCell, rc::Rc};

use super::{theme::*, touch};

pub type Gesture = Rc<RefCell<Option<(String, f32)>>>;

#[derive(Clone, Copy)]
pub enum SliderStyle {
    Standard(Color),
    Scrubber { accent: Color, height: f32 },
}

pub fn drag_value(gesture: &Gesture, id: &str) -> Option<f32> {
    gesture
        .borrow()
        .as_ref()
        .filter(|(track, _)| track == id)
        .map(|(_, value)| *value)
}

pub fn slider(
    theme: Theme,
    id: String,
    value: f32,
    style: SliderStyle,
    enabled: bool,
    gesture: Gesture,
    step: f32,
    change: impl Fn(f32, &mut App) + 'static,
) -> impl IntoElement {
    let (accent, height) = match style {
        SliderStyle::Standard(accent) => (accent, 25.),
        SliderStyle::Scrubber { accent, height } => (accent, height - 2.),
    };
    let change = Rc::new(change);
    let keyboard = change.clone();
    let track_id = id.clone();
    let track = canvas(
        move |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            let scale = f32::from(window.rem_size()) / 16.;
            let (radius, thickness, offset) = match style {
                SliderStyle::Standard(_) => (px(5.), px(4.), px(0.)),
                SliderStyle::Scrubber { .. } => (px(9. * scale), px(6. * scale), px(10. * scale)),
            };
            let left = bounds.left() + radius;
            let width = (bounds.size.width - radius * 2.).max(px(1.));
            let center = bounds.center().y + offset;
            let current = drag_value(&gesture, &track_id)
                .unwrap_or(value)
                .clamp(0., 1.);
            let line = |length: Pixels, color| {
                fill(
                    Bounds::new(
                        point(left, center - thickness / 2.),
                        size(length, thickness),
                    ),
                    color,
                )
                .corner_radii(thickness / 2.)
            };
            window.paint_quad(line(width, theme.color(STROKE)));
            if current > 0. {
                window.paint_quad(line(
                    width * current,
                    theme
                        .color(accent)
                        .opacity(if enabled { 0.85 } else { 0.4 }),
                ));
            }
            if enabled {
                let thumb = Bounds::new(
                    point(left + width * current - radius, center - radius),
                    size(radius * 2., radius * 2.),
                );
                window.paint_quad(fill(thumb, theme.color(accent)).corner_radii(radius));
                window.set_cursor_style(CursorStyle::PointingHand, &hitbox);

                let fraction = move |x: Pixels| ((x - left) / width).clamp(0., 1.);
                let start = gesture.clone();
                let start_id = track_id.clone();
                let start_hitbox = hitbox.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && start_hitbox.is_hovered(window)
                    {
                        *start.borrow_mut() = Some((start_id.clone(), fraction(event.position.x)));
                        window.refresh();
                    }
                });
                let progress = gesture.clone();
                let progress_id = track_id.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    if hitbox.is_hovered(window) {
                        touch::offer_drag(touch::DragTarget::Horizontal);
                    }
                    let mut progress = progress.borrow_mut();
                    if let Some((_, value)) = progress.as_mut().filter(|(id, _)| id == &progress_id)
                    {
                        if event.pressed_button == Some(MouseButton::Left) {
                            *value = fraction(event.position.x);
                        } else {
                            progress.take();
                        }
                        window.refresh();
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture
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
    .h(r(height));

    row()
        .id(gpui::SharedString::from(id))
        .flex_1()
        .border_1()
        .border_color(theme.color(PANEL))
        .when(matches!(style, SliderStyle::Scrubber { .. }), |it| {
            it.border(r(1.))
                .border_color(theme.color(PANEL).opacity(0.))
        })
        .rounded(r(5.))
        .when(enabled, |it| {
            it.tab_index(0)
                .focus(move |it| it.border_color(theme.color(TEAL)))
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
