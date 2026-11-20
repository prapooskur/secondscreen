use std::ops::Range;

use gpui::{
    Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, FocusHandle, HitboxBehavior,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Render, TextRun, Window, canvas, fill,
    point, prelude::*, px, size,
};

use super::theme::*;

pub(super) struct CredentialInput {
    pub value: String,
    focus: FocusHandle,
    masked: bool,
    anchor: usize,
    caret: usize,
}

impl CredentialInput {
    pub fn new(value: String, masked: bool, cx: &mut Context<Self>) -> Self {
        let value = value.trim().to_owned();
        let end = value.len();
        Self {
            value,
            focus: cx.focus_handle().tab_stop(true),
            masked,
            anchor: end,
            caret: end,
        }
    }

    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }

    fn move_caret(&mut self, index: usize, extend: bool) {
        self.caret = index.min(self.value.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    fn replace(&mut self, range: Range<usize>, input: &str) {
        let caret = range.start + input.len();
        self.value.replace_range(range, input);
        self.move_caret(caret, false);
    }

    fn key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let extend = key.modifiers.shift;
        let range = self.selection();
        if key.modifiers.control || key.modifiers.platform {
            match key.key.as_str() {
                "a" => {
                    self.anchor = 0;
                    self.caret = self.value.len();
                }
                "c" | "x" if !self.masked && !range.is_empty() => {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        self.value[range.clone()].to_owned(),
                    ));
                    if key.key == "x" {
                        self.replace(range, "");
                    }
                }
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        let text = text.trim();
                        if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_graphic()) {
                            self.replace(range, text);
                        }
                    }
                }
                _ => return,
            }
        } else if !key.modifiers.alt {
            match key.key.as_str() {
                "left" if !extend && !range.is_empty() => self.move_caret(range.start, false),
                "right" if !extend && !range.is_empty() => self.move_caret(range.end, false),
                "left" => self.move_caret(self.caret.saturating_sub(1), extend),
                "right" => self.move_caret(self.caret + 1, extend),
                "home" => self.move_caret(0, extend),
                "end" => self.move_caret(self.value.len(), extend),
                "backspace" if range.is_empty() => {
                    self.replace(self.caret.saturating_sub(1)..self.caret, "");
                }
                "delete" if range.is_empty() => {
                    self.replace(self.caret..(self.caret + 1).min(self.value.len()), "");
                }
                "backspace" | "delete" => self.replace(range, ""),
                _ => {
                    let Some(text) = key.key_char.as_deref() else {
                        return;
                    };
                    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_graphic()) {
                        return;
                    }
                    self.replace(range, text);
                }
            }
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for CredentialInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = if self.masked {
            "*".repeat(self.value.len())
        } else {
            self.value.clone()
        };
        let caret = self.caret;
        let selection = self.selection();
        let view = cx.entity();
        let focus = self.focus.clone();
        let track = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                let focused = focus.is_focused(window);
                let style = window.text_style();
                let line = window.text_system().shape_line(
                    text.clone().into(),
                    px(14. * f32::from(window.rem_size()) / 16.),
                    &[TextRun {
                        len: text.len(),
                        font: style.font(),
                        color: c(INK),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                );
                let offset = if focused {
                    (line.x_for_index(caret) - bounds.size.width + px(2.)).max(px(0.))
                } else {
                    px(0.)
                };
                let origin = point(bounds.left() - offset, bounds.top());
                if focused {
                    let left = line.x_for_index(selection.start);
                    let right = line.x_for_index(selection.end);
                    let width = if selection.is_empty() {
                        px(1.)
                    } else {
                        right - left
                    };
                    let color = if selection.is_empty() {
                        c(TEAL)
                    } else {
                        c(BLUE).opacity(0.34)
                    };
                    window.paint_quad(fill(
                        Bounds::new(
                            point(origin.x + left, origin.y),
                            size(width, bounds.size.height),
                        ),
                        color,
                    ));
                }
                #[cfg(feature = "ui-gpui")]
                let _ = line.paint(origin, bounds.size.height, window, cx);
                #[cfg(not(feature = "ui-gpui"))]
                let _ = line.paint(
                    origin,
                    bounds.size.height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        let index = line.closest_index_for_x(event.position.x - origin.x);
                        view.update(cx, |input, cx| {
                            input.move_caret(index, event.modifiers.shift);
                            cx.notify();
                        });
                    }
                });
            },
        )
        .w_full()
        .h(r(23.));

        col()
            .id("credential")
            .track_focus(&self.focus)
            .focus(|style| style.border_color(c(TEAL)))
            .on_key_down(cx.listener(Self::key))
            .cursor(CursorStyle::IBeam)
            .justify_center()
            .overflow_hidden()
            .h(r(38.))
            .px(r(10.))
            .rounded(r(7.))
            .border_1()
            .border_color(c(STROKE))
            .bg(c(BACK))
            .font_family("IBM Plex Mono")
            .child(track)
    }
}
