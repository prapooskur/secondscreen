use gpui::{
    AnyElement, App, Div, ElementId, FontWeight, Hsla, IntoElement, Rems, Stateful, Window, div,
    prelude::*, rems, rgb,
};

use super::icons::{Icon, icon};

pub const BACK: u32 = 0x0c1017;
pub const HEADER: u32 = 0x111721;
pub const PANEL: u32 = 0x131b26;
pub const STROKE: u32 = 0x263243;
pub const HOVER: u32 = 0x202c3b;
pub const INK: u32 = 0xf1f5fc;
pub const TEXT: u32 = 0xb1bed0;
pub const MUTED: u32 = 0x8999af;
pub const TEAL: u32 = 0x62ebcb;
pub const BLUE: u32 = 0x83b6ff;
pub const VIOLET: u32 = 0xbca8ff;
pub const AMBER: u32 = 0xf5bd75;
pub const RED: u32 = 0xff8e99;

pub fn r(pixels: f32) -> Rems {
    rems(pixels / 16.)
}

pub fn c(color: u32) -> Hsla {
    rgb(color).into()
}

pub fn row() -> Div {
    div().flex().flex_row().items_center().min_w_0()
}

pub fn split() -> Div {
    div().flex().flex_row().min_w_0()
}

pub fn col() -> Div {
    div().flex().flex_col().min_w_0().min_h_0()
}

pub fn mono(text: impl IntoElement) -> Div {
    div().font_family("IBM Plex Mono").child(text)
}

pub fn quiet(text: impl IntoElement) -> Div {
    div().text_size(r(12.)).text_color(c(MUTED)).child(text)
}

pub fn dot(color: u32) -> Div {
    div().size(r(7.)).rounded_full().bg(c(color)).flex_none()
}

pub fn icon_tile(symbol: Icon, accent: u32, size: f32) -> Div {
    row()
        .size(r(size))
        .flex_none()
        .justify_center()
        .rounded(r(size * 0.25))
        .bg(c(accent).opacity(0.11))
        .child(icon(symbol, c(accent), size * 0.55))
}

pub fn panel() -> Div {
    col()
        .bg(c(PANEL))
        .border_1()
        .border_color(c(STROKE))
        .rounded(r(14.))
}

pub fn title(text: impl IntoElement) -> Div {
    div()
        .text_color(c(INK))
        .text_size(r(23.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

pub fn label(text: impl IntoElement) -> Div {
    div()
        .text_color(c(TEXT))
        .text_size(r(13.))
        .font_weight(FontWeight::MEDIUM)
        .child(text)
}

pub fn chip(text: impl IntoElement, accent: u32) -> Div {
    row()
        .h(r(23.))
        .px(r(8.))
        .rounded(r(5.))
        .bg(c(accent).opacity(0.09))
        .border_1()
        .border_color(c(accent).opacity(0.24))
        .text_size(r(11.))
        .text_color(c(accent))
        .flex_none()
        .child(text)
}

pub fn button(id: impl Into<ElementId>, accent: Option<u32>) -> Stateful<Div> {
    let base = row()
        .id(id)
        .h(r(37.))
        .px(r(12.))
        .gap(r(8.))
        .justify_center()
        .rounded(r(8.))
        .border_1()
        .flex_none()
        .text_size(r(13.))
        .font_weight(FontWeight::MEDIUM);
    match accent {
        Some(accent) => base
            .bg(c(accent).opacity(0.13))
            .border_color(c(accent).opacity(0.33))
            .text_color(c(accent))
            .hover(move |it| it.bg(c(accent).opacity(0.22))),
        None => base
            .bg(c(HEADER))
            .border_color(c(STROKE))
            .text_color(c(TEXT))
            .hover(|it| it.bg(c(HOVER)).text_color(c(INK))),
    }
}

pub fn bind(
    button: Stateful<Div>,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    use std::rc::Rc;
    let action = Rc::new(action);
    let keyboard = action.clone();
    button
        .cursor_pointer()
        .tab_stop(true)
        .focus(|it| it.border_color(c(TEAL)))
        .on_click(move |_, window, cx| action(window, cx))
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") && !event.is_held {
                cx.stop_propagation();
                keyboard(window, cx);
            }
        })
}

pub fn disabled(button: Stateful<Div>) -> Stateful<Div> {
    button.opacity(0.4)
}

pub fn empty(symbol: Icon, accent: u32, heading: &str, help: impl IntoElement) -> Div {
    col()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(r(9.))
        .p(r(14.))
        .child(icon_tile(symbol, accent, 40.))
        .child(
            div()
                .text_size(r(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(heading.to_owned()),
        )
        .child(quiet(help).text_center())
}

pub fn section_header(heading: &str, right: Option<AnyElement>) -> Div {
    row()
        .w_full()
        .justify_between()
        .gap(r(12.))
        .flex_none()
        .child(title(heading.to_owned()))
        .children(right)
}
