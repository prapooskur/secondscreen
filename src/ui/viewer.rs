use super::{Dashboard, text_input::TextInput, theme::*};
use crate::viewer::{Source, ViewerMode, ViewerOptions, WebTile, discord_target};
use gpui::{
    AnyElement, App, Bounds, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Keystroke,
    Modifiers, MouseButton, ObjectFit, PathPromptOptions, Pixels, Point, Render, RenderImage, Task,
    WeakEntity, Window, canvas, div, img, prelude::*, px,
};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

pub(super) struct Viewer {
    id: u32,
    dashboard: WeakEntity<Dashboard>,
    input: Entity<TextInput>,
    options: ViewerOptions,
    mode: ViewerMode,
    interval: u16,
    editing: bool,
    error: Option<String>,
    external: Option<String>,
    web: Option<WebTile>,
    frame: Option<Arc<RenderImage>>,
    pump: Option<Task<()>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    pressed: u32,
    focused: bool,
}

impl Viewer {
    pub(super) fn new(
        options: ViewerOptions,
        dashboard: WeakEntity<Dashboard>,
        id: u32,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::with_placeholder("URL, iframe, channel, or path", cx);
            input.set_value(options.source.clone(), cx);
            input
        });
        cx.on_release(|this, cx| this.clear_frame(cx)).detach();
        let mut this = Self {
            id,
            dashboard,
            input,
            editing: true,
            mode: options.mode,
            interval: options.interval,
            options,
            error: None,
            web: None,
            external: None,
            frame: None,
            pump: None,
            bounds: Rc::default(),
            focus: cx.focus_handle().tab_stop(true),
            pressed: 0,
            focused: false,
        };
        if !this.options.source.is_empty() {
            let result = this
                .options
                .resolve()
                .and_then(|source| this.start(source, cx));
            if let Err(err) = result {
                this.error = Some(err.to_string())
            }
        }
        this
    }

    fn clear_frame(&mut self, cx: &mut App) {
        if let Some(old) = self.frame.take() {
            cx.drop_image(old, None);
        }
    }

    fn viewport(&self) -> (i32, i32) {
        let size = self.bounds.get().size;
        (
            f32::from(size.width).round().clamp(1., 4096.) as i32,
            f32::from(size.height).round().clamp(1., 2160.) as i32,
        )
    }

    fn start(&mut self, source: Source, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let size = self.viewport();
        let web = WebTile::new(&source, if size == (1, 1) { (320, 160) } else { size })?;
        self.external = source.external_url().map(ToString::to_string);
        self.web = Some(web);
        self.editing = false;
        self.pressed = 0;
        self.focused = false;
        self.clear_frame(cx);
        self.pump = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let size = this.viewport();
                        if let Some(web) = &mut this.web
                            && let Some(frame) =
                                web.update(size, this.frame.as_deref(), !this.editing)
                        {
                            this.clear_frame(cx);
                            this.frame = Some(frame);
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(())
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let options = ViewerOptions {
            mode: self.mode,
            source: self.input.read(cx).value.trim().into(),
            interval: self.interval,
        };
        let result = options.resolve().and_then(|source| {
            if let Source::Discord(link) = source {
                self.dashboard
                    .update(cx, |d, cx| d.open_discord(link.as_str(), window, cx))?;
            } else {
                self.start(source, cx)?;
                self.options = options.clone();
                self.dashboard
                    .update(cx, |d, cx| d.set_viewer_options(self.id, options, cx))?;
            }
            Ok(())
        });
        self.error = result.err().map(|e: anyhow::Error| e.to_string());
        cx.notify();
    }

    fn pick(&mut self, cx: &mut Context<Self>) {
        let directory = self.mode == ViewerMode::Slideshow;
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let result = result.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            this.input.update(cx, |input, cx| {
                                input.set_value(path.to_string_lossy().into_owned(), cx)
                            });
                        }
                    }
                    Ok(Err(err)) => this.error = Some(err.to_string()),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn editor(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let mut modes = row().flex_wrap().gap(r(3.));
        for (mode, title) in [
            (ViewerMode::Web, "Web / YouTube"),
            (ViewerMode::TwitchLive, "Twitch Live"),
            (ViewerMode::TwitchChat, "Twitch Chat"),
            (ViewerMode::Media, "Image / video"),
            (ViewerMode::Slideshow, "Slideshow"),
        ] {
            let view = cx.entity();
            modes = modes.child(
                theme.bind(
                    theme
                        .button(title, (self.mode == mode).then_some(BLUE))
                        .h(r(23.))
                        .px(r(5.))
                        .child(title),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.mode = mode;
                            this.error = None;
                            cx.notify()
                        })
                    },
                ),
            );
        }
        let view = cx.entity();
        let mut actions = row().flex_wrap().gap(r(5.)).child(
            theme.bind(
                theme
                    .button("show-source", Some(BLUE))
                    .h(r(27.))
                    .child("Show"),
                move |win, cx| view.update(cx, |v, cx| v.apply(win, cx)),
            ),
        );
        if matches!(self.mode, ViewerMode::Media | ViewerMode::Slideshow) {
            let v = cx.entity();
            actions = actions.child(
                theme.bind(
                    theme
                        .button("choose-media", None)
                        .h(r(27.))
                        .child("Browse…"),
                    move |_, cx| v.update(cx, |v, cx| v.pick(cx)),
                ),
            );
        }
        if self.mode == ViewerMode::Slideshow {
            for (title, delta) in [("−", -1), ("+", 1)] {
                let view = cx.entity();
                actions = actions.child(theme.bind(
                    theme.button(title, None).h(r(25.)).px(r(5.)).child(title),
                    move |_, cx| {
                        view.update(cx, |v, cx| {
                            v.interval = v.interval.saturating_add_signed(delta).clamp(1, 3600);
                            cx.notify()
                        })
                    },
                ));
                if delta < 0 {
                    actions = actions.child(theme.quiet(format!("{} s", self.interval)))
                }
            }
        }
        if let Some(url) = self.external.clone() {
            actions = actions.child(
                theme.bind(
                    theme
                        .button("external", None)
                        .h(r(27.))
                        .px(r(6.))
                        .child("Open in browser"),
                    move |_, cx| cx.open_url(&url),
                ),
            )
        }
        col()
            .id("viewer-editor")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(5.))
            .on_key_down(cx.listener(|v, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" && v.input.read(cx).is_focused(window) {
                    cx.stop_propagation();
                    v.apply(window, cx);
                }
            }))
            .child(modes)
            .child(self.input.clone())
            .child(actions)
            .children(
                self.error
                    .as_ref()
                    .map(|err| theme.quiet(err.clone()).text_color(theme.color(RED))),
            )
    }

    fn event_point(&self, p: Point<Pixels>) -> (f64, f64) {
        let bounds = self.bounds.get();
        let (w, h) = self.viewport();
        let offset = p - bounds.origin;
        (
            f64::from(f32::from(offset.x) / f32::from(bounds.size.width).max(1.)) * f64::from(w),
            f64::from(f32::from(offset.y) / f32::from(bounds.size.height).max(1.)) * f64::from(h),
        )
    }

    fn send_key(&self, key: &Keystroke, down: bool, cx: &mut Context<Self>) {
        if matches!(key.key.as_str(), "escape" | "f11") {
            return;
        }
        let name = match key.key.as_str() {
            "enter" => "Return",
            "space" => "space",
            "tab" => "Tab",
            "backspace" => "BackSpace",
            "delete" => "Delete",
            "left" => "Left",
            "right" => "Right",
            "up" => "Up",
            "down" => "Down",
            "home" => "Home",
            "end" => "End",
            "pageup" => "Page_Up",
            "pagedown" => "Page_Down",
            other => other,
        };
        let text = key.key_char.as_deref().unwrap_or(&key.key);
        let unicode = if text.chars().count() == 1 {
            text.chars()
                .next()
                .filter(|c| !c.is_control())
                .map(u32::from)
                .unwrap_or(0)
        } else {
            0
        };
        if let Some(web) = &self.web {
            web.key(name, unicode, modifiers(key.modifiers), down)
        }
        cx.stop_propagation();
    }

    fn surface(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let bounds = self.bounds.clone();
        let mut tile = div()
            .id("web-surface")
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .bg(theme.color(BACK))
            .track_focus(&self.focus)
            .child(
                canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .when_some(self.frame.clone(), |it, frame| {
                it.child(
                    img(frame)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Fill),
                )
            })
            .on_mouse_move(cx.listener(|v, e: &gpui::MouseMoveEvent, _, _| {
                if let Some(web) = &v.web {
                    web.pointer(
                        0,
                        0,
                        v.event_point(e.position),
                        (0., 0.),
                        v.pressed | modifiers(e.modifiers),
                    )
                }
            }))
            .on_scroll_wheel(cx.listener(|v, e: &gpui::ScrollWheelEvent, _, cx| {
                let d = e.delta.pixel_delta(px(40.));
                if let Some(web) = &v.web {
                    web.pointer(
                        3,
                        0,
                        v.event_point(e.position),
                        (
                            -f64::from(f32::from(d.x)) / 40.,
                            -f64::from(f32::from(d.y)) / 40.,
                        ),
                        modifiers(e.modifiers),
                    );
                }
                cx.stop_propagation()
            }))
            .on_key_down(
                cx.listener(|v, e: &gpui::KeyDownEvent, _, cx| v.send_key(&e.keystroke, true, cx)),
            )
            .on_key_up(
                cx.listener(|v, e: &gpui::KeyUpEvent, _, cx| v.send_key(&e.keystroke, false, cx)),
            );
        for (button, number) in [
            (MouseButton::Left, 1),
            (MouseButton::Middle, 2),
            (MouseButton::Right, 3),
        ] {
            let mask = 1 << (number + 7);
            tile = tile
                .on_mouse_down(
                    button,
                    cx.listener(move |v, e: &gpui::MouseDownEvent, window, cx| {
                        #[cfg(feature = "ui-gpui")]
                        window.focus(&v.focus);
                        #[cfg(not(feature = "ui-gpui"))]
                        window.focus(&v.focus, cx);
                        if let Some(web) = &v.web {
                            if !v.focused {
                                web.focus(true);
                                v.focused = true;
                            }
                            let action = match e.click_count {
                                2 => 4,
                                3 => 5,
                                _ => 1,
                            };
                            web.pointer(
                                action,
                                number,
                                v.event_point(e.position),
                                (0., 0.),
                                v.pressed | modifiers(e.modifiers),
                            );
                        }
                        v.pressed |= mask;
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_up(
                    button,
                    cx.listener(move |v, e: &gpui::MouseUpEvent, _, cx| {
                        v.release(number, e.position, e.modifiers);
                        cx.stop_propagation()
                    }),
                )
                .on_mouse_up_out(
                    button,
                    cx.listener(move |v, e: &gpui::MouseUpEvent, _, _| {
                        v.release(number, e.position, e.modifiers)
                    }),
                );
        }
        tile
    }

    fn release(&mut self, button: i32, point: Point<Pixels>, mods: Modifiers) {
        let mask = 1 << (button + 7);
        if self.pressed & mask != 0 {
            if let Some(web) = &self.web {
                web.pointer(
                    2,
                    button,
                    self.event_point(point),
                    (0., 0.),
                    self.pressed | modifiers(mods),
                )
            }
            self.pressed &= !mask;
        }
    }
}

fn modifiers(m: Modifiers) -> u32 {
    u32::from(m.shift)
        | (u32::from(m.control) << 2)
        | (u32::from(m.alt) << 3)
        | (u32::from(m.platform) << 26)
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = !self.editing && self.focus.is_focused(window);
        if self.focused != focused {
            if let Some(web) = &self.web {
                web.focus(focused)
            }
            self.focused = focused;
        }
        let content: AnyElement = if self.editing {
            self.editor(cx).into_any_element()
        } else {
            self.surface(cx).into_any_element()
        };
        col().flex_1().size_full().child(content)
    }
}

pub(super) fn source_button(view: Entity<Viewer>, cx: &App) -> impl IntoElement {
    let theme = Theme::current(cx);
    let v = view.read(cx);
    theme.bind(
        theme
            .button(("viewer-source", v.id as usize), v.editing.then_some(BLUE))
            .h(r(26.))
            .px(r(8.))
            .child(if v.editing && v.web.is_some() {
                "Back"
            } else {
                "Source"
            }),
        move |_, cx| {
            view.update(cx, |v, cx| {
                v.editing = !(v.editing && v.web.is_some());
                v.mode = v.options.mode;
                v.interval = v.options.interval;
                v.input
                    .update(cx, |i, cx| i.set_value(v.options.source.clone(), cx));
                v.error = None;
                cx.notify();
                let _ = v.dashboard.update(cx, |_, cx| cx.notify());
            })
        },
    )
}

impl Dashboard {
    pub(super) fn open_discord(&mut self, link: &str, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_fullscreen() {
            window.toggle_fullscreen()
        }
        cx.open_url(&discord_target(link));
    }
}
