use gpui::{
    App, Div, ElementId, FontWeight, Global, Hsla, IntoElement, Rems, Stateful, Window,
    WindowAppearance, div, prelude::*, rems, rgb,
};

use crate::config::ThemePreference;

use super::icons::{Icon, icon};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Color {
    dark: u32,
    light: u32,
}

impl Color {
    const fn new(dark: u32, light: u32) -> Self {
        Self { dark, light }
    }
}

pub const BACK: Color = Color::new(0x0c1017, 0xeaf0f7);
pub const HEADER: Color = Color::new(0x111721, 0xf2f6fb);
pub const PANEL: Color = Color::new(0x131b26, 0xffffff);
pub const STROKE: Color = Color::new(0x263243, 0xc3cfde);
pub const HOVER: Color = Color::new(0x202c3b, 0xe2eaf4);
pub const INK: Color = Color::new(0xf1f5fc, 0x152236);
pub const TEXT: Color = Color::new(0xb1bed0, 0x34465e);
pub const MUTED: Color = Color::new(0x8999af, 0x51647d);
pub const TEAL: Color = Color::new(0x62ebcb, 0x086b57);
pub const BLUE: Color = Color::new(0x83b6ff, 0x235caa);
pub const VIOLET: Color = Color::new(0xbca8ff, 0x6042ab);
pub const AMBER: Color = Color::new(0xf5bd75, 0x8b500d);
pub const RED: Color = Color::new(0xff8e99, 0xa62c40);
pub const PINK: Color = Color::new(0xf2799c, 0xa33864);
pub const SPEAKING_GREEN: Color = Color::new(0x23a55a, 0x16733e);
pub const SNOW: Color = Color::new(0xbce9ff, 0x246783);
pub const ARTWORK_BACK: Color = Color::new(0x172842, 0xe7f0fc);
pub const SCRIM: Color = Color::new(0x0c1017, 0x60728b);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Global for Theme {}

impl Theme {
    pub fn apply(preference: ThemePreference, appearance: WindowAppearance, cx: &mut App) {
        let theme = match preference {
            ThemePreference::Light => Self::Light,
            ThemePreference::Dark => Self::Dark,
            ThemePreference::System => match appearance {
                WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
            },
        };
        if cx.try_global::<Self>() != Some(&theme) {
            cx.set_global(theme);
            cx.refresh_windows();
        }
    }

    pub fn current(cx: &App) -> Self {
        *cx.global::<Self>()
    }

    pub fn color(self, color: Color) -> Hsla {
        rgb(match self {
            Self::Dark => color.dark,
            Self::Light => color.light,
        })
        .into()
    }

    pub fn quiet(self, text: impl IntoElement) -> Div {
        div()
            .text_size(r(12.))
            .text_color(self.color(MUTED))
            .child(text)
    }

    pub fn dot(self, color: Color) -> Div {
        div()
            .size(r(7.))
            .rounded_full()
            .bg(self.color(color))
            .flex_none()
    }

    pub fn icon_tile(self, symbol: Icon, accent: Color, size: f32) -> Div {
        row()
            .size(r(size))
            .flex_none()
            .justify_center()
            .rounded(r(size * 0.25))
            .bg(self.color(accent).opacity(0.11))
            .child(icon(symbol, self.color(accent), size * 0.55))
    }

    pub fn panel(self) -> Div {
        col()
            .bg(self.color(PANEL))
            .border_1()
            .border_color(self.color(STROKE))
            .rounded(r(14.))
    }

    pub fn title(self, text: impl IntoElement) -> Div {
        div()
            .text_color(self.color(INK))
            .text_size(r(23.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text)
    }

    pub fn label(self, text: impl IntoElement) -> Div {
        div()
            .text_color(self.color(TEXT))
            .text_size(r(13.))
            .font_weight(FontWeight::MEDIUM)
            .child(text)
    }

    pub fn chip(self, text: impl IntoElement, accent: Color) -> Div {
        row()
            .h(r(23.))
            .px(r(8.))
            .rounded(r(5.))
            .bg(self.color(accent).opacity(0.09))
            .border_1()
            .border_color(self.color(accent).opacity(0.24))
            .text_size(r(11.))
            .text_color(self.color(accent))
            .flex_none()
            .child(text)
    }

    pub fn button(self, id: impl Into<ElementId>, accent: Option<Color>) -> Stateful<Div> {
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
                .bg(self.color(accent).opacity(0.13))
                .border_color(self.color(accent).opacity(0.33))
                .text_color(self.color(accent))
                .hover(move |it| it.bg(self.color(accent).opacity(0.22))),
            None => base
                .bg(self.color(HEADER))
                .border_color(self.color(STROKE))
                .text_color(self.color(TEXT))
                .hover(move |it| it.bg(self.color(HOVER)).text_color(self.color(INK))),
        }
    }

    pub fn bind(
        self,
        button: Stateful<Div>,
        action: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        button
            .cursor_pointer()
            .tab_index(0)
            .focus(move |it| it.border_color(self.color(TEAL)))
            .on_click(move |_, window, cx| action(window, cx))
    }

    pub fn empty(self, symbol: Icon, accent: Color, heading: &str, help: impl IntoElement) -> Div {
        col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(r(9.))
            .p(r(14.))
            .child(self.icon_tile(symbol, accent, 40.))
            .child(
                div()
                    .text_size(r(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(heading.to_owned()),
            )
            .child(self.quiet(help).text_center())
    }
}

pub fn r(pixels: f32) -> Rems {
    rems(pixels / 16.)
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

pub fn disabled(button: Stateful<Div>) -> Stateful<Div> {
    button.opacity(0.4)
}
