use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, HitboxBehavior, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Styled, canvas, fill, point,
    prelude::*, px, size,
};
use std::{cell::RefCell, rc::Rc};

use super::{theme::*, touch};

pub type Gesture = Rc<RefCell<Option<(String, f32)>>>;

#[derive(Clone, Copy)]
pub enum SliderStyle {
    Standard(Color),
    Scrubber { accent: Color, height: f32 },
    Fader(Color),
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
        SliderStyle::Standard(accent) | SliderStyle::Fader(accent) => (accent, 25.),
        SliderStyle::Scrubber { accent, height } => (accent, height - 2.),
    };
    let vertical = matches!(style, SliderStyle::Fader(_));
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
                SliderStyle::Fader(_) => (px(4. * scale), px(12. * scale), px(0.)),
            };
            let (start, length, center) = if vertical {
                (
                    bounds.bottom() - radius,
                    bounds.size.height,
                    bounds.center().x,
                )
            } else {
                (
                    bounds.left() + radius,
                    bounds.size.width,
                    bounds.center().y + offset,
                )
            };
            let length = (length - radius * 2.).max(px(1.));
            let current = drag_value(&gesture, &track_id)
                .unwrap_or(value)
                .clamp(0., 1.);
            let line = |length: Pixels, color| {
                let bounds = if vertical {
                    Bounds::new(
                        point(center - thickness / 2., start - length),
                        size(thickness, length),
                    )
                } else {
                    Bounds::new(
                        point(start, center - thickness / 2.),
                        size(length, thickness),
                    )
                };
                fill(bounds, color).corner_radii(thickness / 2.)
            };
            window.paint_quad(line(length, theme.color(STROKE)));
            if current > 0. {
                window.paint_quad(line(
                    length * current,
                    theme
                        .color(accent)
                        .opacity(if enabled { 0.85 } else { 0.4 }),
                ));
            }
            if enabled {
                let (thumb, rounding) = if vertical {
                    let half_width = px(12. * scale);
                    (
                        Bounds::new(
                            point(center - half_width, start - length * current - radius),
                            size(half_width * 2., radius * 2.),
                        ),
                        px(2. * scale),
                    )
                } else {
                    (
                        Bounds::new(
                            point(start + length * current - radius, center - radius),
                            size(radius * 2., radius * 2.),
                        ),
                        radius,
                    )
                };
                window.paint_quad(fill(thumb, theme.color(accent)).corner_radii(rounding));
                window.set_cursor_style(CursorStyle::PointingHand, &hitbox);

                let fraction = move |position: Point<Pixels>| {
                    let distance = if vertical {
                        start - position.y
                    } else {
                        position.x - start
                    };
                    (distance / length).clamp(0., 1.)
                };
                let start = gesture.clone();
                let start_id = track_id.clone();
                let start_hitbox = hitbox.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && start_hitbox.is_hovered(window)
                    {
                        *start.borrow_mut() = Some((start_id.clone(), fraction(event.position)));
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
                        touch::offer_drag(if vertical {
                            touch::DragTarget::Vertical
                        } else {
                            touch::DragTarget::Horizontal
                        });
                    }
                    let mut progress = progress.borrow_mut();
                    if let Some((_, value)) = progress.as_mut().filter(|(id, _)| id == &progress_id)
                    {
                        if event.pressed_button == Some(MouseButton::Left) {
                            *value = fraction(event.position);
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
                        change(fraction(event.position), cx);
                        window.refresh();
                    }
                });
            }
        },
    )
    .w_full()
    .when_else(vertical, |it| it.h_full(), |it| it.h(r(height)));

    row()
        .id(gpui::SharedString::from(id))
        .flex_1()
        .when(vertical, |it| it.w(r(42.)).min_h_0())
        .border_1()
        .border_color(theme.color(PANEL))
        .when(!matches!(style, SliderStyle::Standard(_)), |it| {
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
