use std::ops::Range;

use gpui::{
    Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, Entity, FocusHandle,
    HitboxBehavior, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Render, TextRun,
    Window, canvas, fill, point, prelude::*, px, size,
};

use super::theme::*;

pub(super) fn field(
    theme: Theme,
    name: &'static str,
    input: Entity<TextInput>,
) -> impl IntoElement {
    col()
        .id(name)
        .flex_1()
        .gap(r(4.))
        .child(theme.label(name))
        .child(input)
}

pub(super) struct TextInput {
    pub value: String,
    focus: FocusHandle,
    kind: InputKind,
    anchor: usize,
    caret: usize,
}

enum InputKind {
    Credential { masked: bool },
    Plain { placeholder: &'static str },
}

impl TextInput {
    pub fn new(value: String, masked: bool, cx: &mut Context<Self>) -> Self {
        let value = value.trim().to_owned();
        let end = value.len();
        Self {
            value,
            focus: cx.focus_handle().tab_stop(true),
            kind: InputKind::Credential { masked },
            anchor: end,
            caret: end,
        }
    }

    pub fn with_placeholder(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        Self {
            kind: InputKind::Plain { placeholder },
            ..Self::new(String::new(), false, cx)
        }
    }

    pub fn set_value(&mut self, value: String, cx: &mut Context<Self>) {
        self.value = value;
        self.move_caret(self.value.len(), false);
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    fn is_masked(&self) -> bool {
        matches!(self.kind, InputKind::Credential { masked: true })
    }

    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }

    fn move_caret(&mut self, index: usize, extend: bool) {
        self.caret = self.value.floor_char_boundary(index);
        if !extend {
            self.anchor = self.caret;
        }
    }

    fn replace(&mut self, range: Range<usize>, input: &str) {
        let caret = range.start + input.len();
        self.value.replace_range(range, input);
        self.move_caret(caret, false);
    }

    fn previous(&self) -> usize {
        self.value[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next(&self) -> usize {
        self.caret
            + self.value[self.caret..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
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
                "c" | "x" if !self.is_masked() && !range.is_empty() => {
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
                        if matches!(self.kind, InputKind::Plain { .. }) {
                            let line: String = text
                                .chars()
                                .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
                                .filter(|ch| !ch.is_control())
                                .collect();
                            if !line.is_empty() {
                                self.replace(range, &line);
                            }
                        } else if !text.is_empty()
                            && text.bytes().all(|byte| byte.is_ascii_graphic())
                        {
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
                "left" => self.move_caret(self.previous(), extend),
                "right" => self.move_caret(self.next(), extend),
                "home" => self.move_caret(0, extend),
                "end" => self.move_caret(self.value.len(), extend),
                "backspace" if range.is_empty() => {
                    self.replace(self.previous()..self.caret, "");
                }
                "delete" if range.is_empty() => {
                    self.replace(self.caret..self.next(), "");
                }
                "backspace" | "delete" => self.replace(range, ""),
                _ => {
                    let Some(text) = key.key_char.as_deref() else {
                        return;
                    };
                    let valid = if matches!(self.kind, InputKind::Plain { .. }) {
                        text.chars()
                            .all(|ch| !ch.is_control() && (!ch.is_whitespace() || ch == ' '))
                    } else {
                        text.bytes().all(|byte| byte.is_ascii_graphic())
                    };
                    if text.is_empty() || !valid {
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

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let placeholder = match self.kind {
            InputKind::Plain { placeholder } if self.value.is_empty() => Some(placeholder),
            _ => None,
        };
        let text = if let Some(placeholder) = placeholder {
            placeholder.to_owned()
        } else if self.is_masked() {
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
                        color: theme.color(if placeholder.is_some() { MUTED } else { INK }),
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
                        theme.color(TEAL)
                    } else {
                        theme.color(BLUE).opacity(0.34)
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
            .id("text-input")
            .track_focus(&self.focus)
            .focus(move |style| style.border_color(theme.color(TEAL)))
            .on_key_down(cx.listener(Self::key))
            .cursor(CursorStyle::IBeam)
            .justify_center()
            .overflow_hidden()
            .h(r(38.))
            .px(r(10.))
            .rounded(r(7.))
            .border_1()
            .border_color(theme.color(STROKE))
            .bg(theme.color(BACK))
            .font_family("IBM Plex Mono")
            .child(track)
    }
}
