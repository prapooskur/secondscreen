mod layout;
mod options;
mod performance;
mod profiles;
mod widgets;

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, DispatchPhase, HitboxBehavior, IntoElement, MouseButton, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollHandle, ScrollWheelEvent, Window, canvas, div, point,
    prelude::*, px,
};

pub(super) use self::layout::Kind;
use self::layout::{Layout, MAX_COLUMNS, MIN_ROWS, Placement, VISIBLE_ROWS, Widget, shifted_span};
pub(super) use self::profiles::{Badge, import_profile_image};
use self::profiles::{Profile, Profiles};
use super::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    theme::*,
};

const GAP: f32 = 12.;
const REFERENCE_ROWS: u8 = 3;
const REFERENCE_COLUMNS: u8 = 4;
const MIN_COLUMN_WIDTH: f32 = 232.;
const RAIL_WIDTH: f32 = 44.;
const RAIL_BUTTON_SIZE: f32 = 34.;
const WIDGET_CONTROL_HEIGHT: f32 = 28.;

pub(super) struct Home {
    profiles: Profiles,
    sensor_editor: Option<(u32, usize)>,
    path: Option<PathBuf>,
    editing: bool,
    catalog: bool,
    resizing: Option<Resize>,
    scroll: ScrollHandle,
}

struct Resize {
    original: Widget,
    preview: Widget,
    origin: Point<Pixels>,
    grid: Grid,
    scale: f32,
}

#[derive(Clone, Copy)]
struct Grid {
    columns: u8,
    width: f32,
    height: f32,
}

#[derive(Clone, Copy)]
struct HorizontalScroll {
    step: Pixels,
    limit: Pixels,
}

impl Grid {
    fn new(available_width: f32, available_height: f32) -> Self {
        let available_width = available_width.max(1.);
        let fit_columns = |width: f32| {
            (((available_width + GAP) / (width + GAP)).floor() as u8).clamp(1, MAX_COLUMNS)
        };
        let row_height = |rows| ((available_height + GAP) / f32::from(rows) - GAP).max(1.);
        let reference_columns = fit_columns(MIN_COLUMN_WIDTH).min(REFERENCE_COLUMNS);
        let reference_width = (available_width + GAP) / f32::from(reference_columns) - GAP;
        let height = row_height(VISIBLE_ROWS);
        let reference_height = row_height(REFERENCE_ROWS).clamp(80., 118.);
        let width = (reference_width * height / reference_height)
            .clamp(MIN_COLUMN_WIDTH.min(available_width), available_width);
        Self {
            columns: fit_columns(width),
            width,
            height,
        }
    }

    fn span_width(self, columns: usize) -> f32 {
        columns as f32 * (self.width + GAP) - GAP
    }

    fn span_height(self, rows: usize) -> f32 {
        rows as f32 * (self.height + GAP) - GAP
    }
}

impl Home {
    pub(super) fn load() -> Self {
        let path = crate::config::path("home.json");
        let profiles = path
            .as_deref()
            .map(Profiles::read)
            .transpose()
            .unwrap_or_else(|error| {
                log::warn!("Could not load the Home profiles: {error:#}");
                None
            })
            .unwrap_or_default();
        Self {
            profiles,
            sensor_editor: None,
            path,
            editing: false,
            catalog: false,
            resizing: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub(super) fn contains(&self, kind: Kind) -> bool {
        self.layout()
            .widgets
            .iter()
            .any(|widget| widget.kind == kind)
    }

    pub(super) fn shows(&self, kind: Kind) -> bool {
        !self.catalog && self.contains(kind)
    }

    pub(super) fn shows_media_progress(&self) -> bool {
        !self.catalog
            && self
                .widgets()
                .any(|widget| widget.kind == Kind::Media && widget.rows >= 3)
    }

    pub(super) fn active_profile(&self) -> (usize, &Profile) {
        let index = self.profiles.active;
        (index, &self.profiles.items[index])
    }

    fn layout(&self) -> &Layout {
        &self.active_profile().1.layout
    }

    fn layout_mut(&mut self) -> &mut Layout {
        &mut self.profiles.items[self.profiles.active].layout
    }

    pub(super) fn leave(&mut self) {
        self.sensor_editor = None;
        self.editing = false;
        self.catalog = false;
        self.resizing = None;
    }

    pub(super) fn escape(&mut self) -> bool {
        if self.sensor_editor.take().is_some() || self.resizing.take().is_some() {
            return true;
        }
        if self.catalog {
            self.catalog = false;
            return true;
        }
        if self.editing {
            self.leave();
            return true;
        }
        false
    }

    fn widgets(&self) -> impl Iterator<Item = Widget> + '_ {
        self.layout().widgets.iter().map(|widget| {
            self.resizing
                .as_ref()
                .filter(|drag| drag.original.id == widget.id)
                .map_or(*widget, |drag| drag.preview)
        })
    }

    fn resize(&mut self, next: Widget) -> bool {
        let Some(widget) = self
            .layout_mut()
            .widgets
            .iter_mut()
            .find(|widget| widget.id == next.id)
        else {
            return false;
        };
        if *widget == next {
            return false;
        }
        *widget = next;
        true
    }
}

impl Dashboard {
    pub(super) fn home_page(
        &self,
        width: f32,
        height: f32,
        scale: f32,
        fullscreen: bool,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let viewport_width = (width - 2. * (RAIL_WIDTH + GAP)).max(1.);
        let grid = Grid::new(viewport_width, height);
        let mut scrolling = None;
        let content = if self.home.catalog {
            self.widget_catalog(grid, cx).into_any_element()
        } else {
            let placements = layout::place(self.home.widgets(), grid.columns);
            if placements.is_empty() {
                self.empty_home(cx).into_any_element()
            } else {
                let columns = placements
                    .iter()
                    .map(|tile| tile.x + usize::from(tile.columns))
                    .max()
                    .unwrap_or_default();
                let visible_width = grid.span_width(usize::from(grid.columns));
                let content_width = grid.span_width(columns).max(visible_width);
                let overflow = px((content_width - viewport_width) * scale);
                if overflow > px(0.5) {
                    scrolling = Some(HorizontalScroll {
                        step: px((visible_width + GAP) * scale),
                        limit: overflow,
                    });
                }
                let count = placements.len();
                let viewport = col()
                    .id("home-widget-scroll")
                    .flex_1()
                    .overflow_hidden()
                    .track_scroll(&self.home.scroll)
                    .when(scrolling.is_none(), |it| it.items_center())
                    .child(
                        div()
                            .relative()
                            .w(r(content_width))
                            .h(r(grid.span_height(usize::from(VISIBLE_ROWS))))
                            .flex_none()
                            .children(placements.into_iter().enumerate().map(|(index, tile)| {
                                self.widget_tile(tile, index, count, grid, scale, cx)
                            })),
                    );
                col()
                    .flex_1()
                    .relative()
                    .child(viewport)
                    .children(scrolling.map(|scroll| self.track_home_scroll(scroll.limit)))
                    .into_any_element()
            }
        };

        split()
            .relative()
            .flex_1()
            .min_h_0()
            .gap(r(GAP))
            .child(self.profile_rail(cx))
            .child(
                col()
                    .id(("profile-home", self.home.profiles.active))
                    .flex_1()
                    .child(content),
            )
            .child(self.control_rail(fullscreen, scrolling, cx))
            .when(self.home.resizing.is_some(), |it| {
                it.child(self.track_resize(cx))
            })
    }

    fn control_rail(
        &self,
        fullscreen: bool,
        scrolling: Option<HorizontalScroll>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let editing = self.home.editing;
        let catalog = self.home.catalog;
        let edit_view = cx.entity();
        let settings_view = cx.entity();
        let fullscreen_view = cx.entity();

        theme
            .panel()
            .w(r(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .items_center()
            .gap(r(5.))
            .py(r(5.))
            .child(rail_control(
                theme,
                "edit-home",
                if editing {
                    "Done editing"
                } else {
                    "Edit layout"
                },
                if editing { Icon::Check } else { Icon::Edit },
                editing,
                move |window, cx| {
                    edit_view.update(cx, |this, cx| {
                        this.close_settings(window, cx);
                        if this.home.editing {
                            this.home.leave();
                        } else {
                            this.home.editing = true;
                        }
                        cx.notify();
                    });
                },
            ))
            .when(editing, |it| {
                let view = cx.entity();
                it.child(rail_control(
                    theme,
                    "widget-catalog",
                    if catalog {
                        "Back to layout"
                    } else {
                        "Add widget"
                    },
                    if catalog { Icon::Left } else { Icon::Plus },
                    catalog,
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.home.catalog = !this.home.catalog;
                            this.home.resizing = None;
                            cx.notify();
                        })
                    },
                ))
            })
            .child(rail_control(
                theme,
                "settings",
                "Settings",
                Icon::Settings,
                self.settings.open,
                move |window, cx| {
                    settings_view.update(cx, |this, cx| {
                        if this.settings.open {
                            this.close_settings(window, cx);
                        } else {
                            this.open_settings(this.settings.section, window, cx);
                        }
                    })
                },
            ))
            .child(
                col()
                    .flex_1()
                    .w_full()
                    .justify_center()
                    .items_center()
                    .gap(r(5.))
                    .children(scrolling.map(|scroll| self.scroll_controls(theme, scroll)))
                    .when(!fullscreen, |it| {
                        it.child(
                            row()
                                .id("move-window")
                                .size(r(26.))
                                .justify_center()
                                .rounded(r(8.))
                                .cursor_move()
                                .hover(move |it| it.bg(theme.color(HOVER)))
                                .child(icon(Icon::Grip, theme.color(MUTED), 16.))
                                .tooltip(|_, cx| {
                                    cx.new(|_| Tooltip("Drag to move window".into())).into()
                                })
                                .on_mouse_down(MouseButton::Left, |event, window, _| {
                                    if event.click_count == 2 {
                                        window.zoom_window();
                                    } else {
                                        window.start_window_move();
                                    }
                                }),
                        )
                    }),
            )
            .child(rail_control(
                theme,
                "fullscreen",
                if fullscreen {
                    "Exit fullscreen (F11)"
                } else {
                    "Fullscreen (F11)"
                },
                if fullscreen {
                    Icon::Restore
                } else {
                    Icon::Fullscreen
                },
                false,
                move |window, cx| {
                    fullscreen_view.update(cx, |this, _| this.toggle_fullscreen(window))
                },
            ))
            .when(!fullscreen, |it| {
                it.child(rail_control(
                    theme,
                    "minimize",
                    "Minimize",
                    Icon::Minimize,
                    false,
                    |window, _| window.minimize_window(),
                ))
                .child(rail_control(
                    theme,
                    "close",
                    "Close",
                    Icon::Close,
                    false,
                    |window, _| window.remove_window(),
                ))
            })
    }

    fn scroll_controls(&self, theme: Theme, scroll: HorizontalScroll) -> impl IntoElement {
        let offset = self.home.scroll.offset().x;
        col().flex_none().gap(r(5.)).children(
            [
                ("Scroll left", Icon::Left, scroll.step, offset < px(-0.5)),
                (
                    "Scroll right",
                    Icon::Chevron,
                    -scroll.step,
                    offset > -scroll.limit + px(0.5),
                ),
            ]
            .map(|(name, symbol, step, enabled)| {
                let control = tile_button(theme, name, name, symbol, TEXT).size(r(28.));
                if !enabled {
                    return disabled(control);
                }
                let handle = self.home.scroll.clone();
                theme.bind(control, move |window, _| {
                    scroll_horizontally(&handle, step, scroll.limit, window)
                })
            }),
        )
    }

    fn track_home_scroll(&self, limit: Pixels) -> impl IntoElement {
        let handle = self.home.scroll.clone();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, _| {
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !hitbox.should_handle_scroll(window) {
                        return;
                    }
                    let delta = event.delta.pixel_delta(window.line_height());
                    let step = if event.modifiers.shift && delta.x == px(0.) {
                        delta.y
                    } else if delta.x.abs() >= delta.y.abs() {
                        delta.x
                    } else {
                        return;
                    };
                    if step != px(0.) {
                        scroll_horizontally(&handle, step, limit, window);
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    fn empty_home(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity();
        theme
            .panel()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(r(12.))
            .child(theme.icon_tile(Icon::Home, TEAL, 44.))
            .child(theme.title("Your Home is empty"))
            .child(theme.bind(
                theme.button("first-widget", Some(TEAL)).child("Add widget"),
                move |window, cx| {
                    view.update(cx, |this, cx| {
                        this.close_settings(window, cx);
                        this.home.editing = true;
                        this.focus_dashboard(window, cx);
                        this.home.catalog = true;
                        cx.notify();
                    });
                },
            ))
    }

    fn widget_catalog(&self, grid: Grid, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let available = Kind::ALL
            .into_iter()
            .filter(|kind| kind.repeatable() || !self.home.contains(*kind))
            .collect::<Vec<_>>();

        col()
            .id("widget-catalog-scroll")
            .flex_1()
            .items_center()
            .overflow_y_scroll()
            .when(available.is_empty(), |it| {
                it.child(theme.quiet("All widgets are on Home.").p(r(6.)))
            })
            .child(
                split()
                    .w(r(grid.span_width(usize::from(grid.columns))))
                    .flex_wrap()
                    .gap(r(GAP))
                    .children(available.into_iter().map(|kind| {
                        let view = cx.entity();
                        theme.bind(
                            theme
                                .button(("add-widget", kind as usize), None)
                                .w(r(grid.width))
                                .h(r(77.))
                                .justify_start()
                                .gap(r(11.))
                                .child(theme.icon_tile(kind.icon(), kind.accent(), 38.))
                                .child(
                                    col()
                                        .flex_1()
                                        .gap(r(3.))
                                        .child(
                                            theme
                                                .label(kind.name())
                                                .text_color(theme.color(INK))
                                                .truncate(),
                                        )
                                        .child(theme.quiet(kind.description()).truncate()),
                                )
                                .child(icon(Icon::Plus, theme.color(kind.accent()), 15.)),
                            move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.focus_dashboard(window, cx);
                                    this.home.layout_mut().add(kind);
                                    this.save_home(cx);
                                })
                            },
                        )
                    })),
            )
    }

    fn widget_tile(
        &self,
        tile: Placement,
        index: usize,
        count: usize,
        grid: Grid,
        scale: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let kind = tile.widget.kind;
        let editing = self.home.editing;
        let height = grid.span_height(usize::from(tile.rows));
        let width = grid.span_width(usize::from(tile.columns));
        let (padding, gap) = if height < 155. { (8., 6.) } else { (13., 11.) };
        let mut content_height = height - 2. * (padding + 1.);
        if editing {
            content_height -= 2. * (WIDGET_CONTROL_HEIGHT + gap);
        }

        theme
            .panel()
            .absolute()
            .top(r(tile.y as f32 * (grid.height + GAP)))
            .left(r(tile.x as f32 * (grid.width + GAP)))
            .w(r(width))
            .h(r(height))
            .p(r(padding))
            .gap(r(gap))
            .overflow_hidden()
            .when(editing, |it| {
                it.border_color(theme.color(kind.accent()).opacity(0.42))
                    .child(self.widget_edit_header(tile.widget, index, count, cx))
            })
            .child(col().flex_1().overflow_hidden().child(self.widget_content(
                tile,
                width - 2. * (padding + 1.),
                content_height,
                cx,
            )))
            .when(editing, |it| {
                it.child(
                    row()
                        .justify_between()
                        .gap(r(6.))
                        .flex_none()
                        .child(self.widget_size_controls(tile, grid.columns, cx))
                        .child(self.resize_handle(tile.widget, grid, scale, cx)),
                )
            })
    }

    fn widget_control(&self, widget: Widget, cx: &Context<Self>) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        let name = match widget.kind {
            Kind::Performance => return Some(self.sensor_button(widget.id, cx).into_any_element()),
            Kind::Notifications => return Some(self.clear_notifications(cx).into_any_element()),
            Kind::Viewer => {
                return self.viewers.get(&widget.id).map(|viewer| {
                    super::viewer::source_button(viewer.clone(), cx).into_any_element()
                });
            }
            Kind::Media => return self.player_control(cx),
            Kind::WorldClock => "Zones",
            Kind::Weather => "Location",
            Kind::Fps => "Setup",
            _ => return None,
        };
        let view = cx.entity();
        Some(
            theme
                .bind(
                    theme.button(name, None).h(r(26.)).px(r(7.)).child(name),
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            this.open_settings(super::settings::Section::Widgets, window, cx)
                        })
                    },
                )
                .into_any_element(),
        )
    }

    fn widget_edit_header(
        &self,
        widget: Widget,
        index: usize,
        count: usize,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let id = widget.id;
        let name = widget.kind.name();
        let view = cx.entity();
        row()
            .h(r(WIDGET_CONTROL_HEIGHT))
            .flex_none()
            .gap(r(4.))
            .child(
                row()
                    .flex_1()
                    .gap(r(4.))
                    .child(
                        theme
                            .label(name)
                            .id(("widget-name", id as usize))
                            .text_color(theme.color(INK))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .tooltip(move |_, cx| cx.new(|_| Tooltip(name.into())).into()),
                    )
                    .children(self.widget_control(widget, cx)),
            )
            .child(self.widget_move(id, -1, index > 0, cx))
            .child(self.widget_move(id, 1, index + 1 < count, cx))
            .child(
                theme.bind(
                    tile_button(
                        theme,
                        ("remove-widget", id as usize),
                        "Remove widget",
                        Icon::Close,
                        RED,
                    )
                    .w(r(23.)),
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            this.focus_dashboard(window, cx);
                            this.home.resizing = None;
                            let layout = this.home.layout_mut();
                            layout.widgets.retain(|widget| widget.id != id);
                            layout.sensors.remove(&id);
                            layout.viewers.remove(&id);
                            this.viewers.remove(&id);
                            super::monitor::discard_charts(Some(id), window, cx);
                            this.home.sensor_editor = None;
                            this.save_home(cx);
                        })
                    },
                ),
            )
    }

    fn widget_move(
        &self,
        id: u32,
        direction: isize,
        enabled: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (name, symbol) = if direction < 0 {
            ("Move earlier", Icon::Left)
        } else {
            ("Move later", Icon::Chevron)
        };
        let control = tile_button(theme, (name, id as usize), name, symbol, TEXT).w(r(23.));
        if !enabled {
            return disabled(control);
        }
        let view = cx.entity();
        theme.bind(control, move |_, cx| {
            view.update(cx, |this, cx| {
                this.home.resizing = None;
                let widgets = &mut this.home.layout_mut().widgets;
                if let Some(index) = widgets.iter().position(|widget| widget.id == id)
                    && let Some(next) = index
                        .checked_add_signed(direction)
                        .filter(|&next| next < widgets.len())
                {
                    widgets.swap(index, next);
                    this.save_home(cx);
                }
            })
        })
    }

    fn resize_handle(
        &self,
        widget: Widget,
        grid: Grid,
        scale: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let pointer = cx.entity();
        let keyboard = pointer.clone();
        tile_button(
            theme,
            ("resize-widget", widget.id as usize),
            "Drag or use arrow keys to resize",
            Icon::Resize,
            TEAL,
        )
        .cursor_nwse_resize()
        .tab_index(0)
        .focus(move |it| it.border_color(theme.color(TEAL)))
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            pointer.update(cx, |this, cx| {
                this.home.resizing = Some(Resize {
                    original: widget,
                    preview: widget,
                    origin: event.position,
                    grid,
                    scale,
                });
                cx.notify();
            });
        })
        .on_key_down(move |event, _, cx| {
            let (horizontal, vertical) = match event.keystroke.key.as_str() {
                "left" => (-1, 0),
                "right" => (1, 0),
                "up" => (0, -1),
                "down" => (0, 1),
                _ => return,
            };
            cx.stop_propagation();
            keyboard.update(cx, |this, cx| {
                this.adjust_widget(widget.id, grid.columns, (horizontal, vertical), cx);
            });
        })
    }

    fn widget_size_controls(
        &self,
        tile: Placement,
        columns: u8,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let widget = tile.widget;
        let step = |description: &'static str, symbol, horizontal, vertical| {
            let delta = (horizontal, vertical);
            let control = tile_button(
                theme,
                (description, widget.id as usize),
                description,
                symbol,
                TEXT,
            )
            .w(r(23.))
            .h(r(24.));
            if adjusted(widget, columns, delta) == widget {
                return disabled(control);
            }
            let view = cx.entity();
            theme.bind(control, move |_, cx| {
                view.update(cx, |this, cx| {
                    this.adjust_widget(widget.id, columns, delta, cx)
                })
            })
        };
        let number = |value: u8| {
            mono(value.to_string())
                .w(r(13.))
                .text_center()
                .text_size(r(12.))
        };

        row()
            .gap(r(3.))
            .child(step("Narrower", Icon::Minimize, -1, 0))
            .child(number(tile.columns))
            .child(step("Wider", Icon::Plus, 1, 0))
            .child(theme.quiet("×").px(r(2.)))
            .child(step("Shorter", Icon::Minimize, 0, -1))
            .child(number(tile.rows))
            .child(step("Taller", Icon::Plus, 0, 1))
    }

    fn adjust_widget(&mut self, id: u32, columns: u8, delta: (i32, i32), cx: &mut Context<Self>) {
        if let Some(current) = self.home.layout().widgets.iter().find(|item| item.id == id)
            && self.home.resize(adjusted(*current, columns, delta))
        {
            self.save_home(cx);
        }
    }

    fn track_resize(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let movement = view.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    movement.update(cx, |this, cx| {
                        let Some(drag) = &mut this.home.resizing else {
                            return;
                        };
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.home.resizing = None;
                            cx.notify();
                            return;
                        }
                        let offset = event.position - drag.origin;
                        let width = (f32::from(offset.x) / ((drag.grid.width + GAP) * drag.scale))
                            .round() as i32;
                        let height = (f32::from(offset.y) / ((drag.grid.height + GAP) * drag.scale))
                            .round() as i32;
                        let next = adjusted(drag.original, drag.grid.columns, (width, height));
                        if drag.preview != next {
                            drag.preview = next;
                            cx.notify();
                        }
                    });
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                        return;
                    }
                    if view.update(cx, |this, cx| {
                        let Some(drag) = this.home.resizing.take() else {
                            return false;
                        };
                        if this.home.resize(drag.preview) {
                            this.save_home(cx);
                        } else {
                            cx.notify();
                        }
                        true
                    }) {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    pub(super) fn sync_viewers(&mut self, cx: &mut Context<Self>) {
        if self.home.catalog {
            for viewer in self.viewers.values() {
                viewer.read(cx).hide();
            }
            return;
        }
        for widget in self
            .home
            .layout()
            .widgets
            .iter()
            .filter(|w| w.kind == Kind::Viewer)
        {
            let id = widget.id;
            self.viewers.entry(id).or_insert_with(|| {
                let options = self
                    .home
                    .layout()
                    .viewers
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                let dashboard = cx.weak_entity();
                cx.new(|cx| super::viewer::Viewer::new(options, dashboard, id, cx))
            });
        }
    }

    pub(super) fn set_viewer_options(
        &mut self,
        id: u32,
        options: crate::viewer::ViewerOptions,
        cx: &mut Context<Self>,
    ) {
        self.home.layout_mut().viewers.insert(id, options);
        self.save_home(cx);
    }

    fn save_home(&mut self, cx: &mut Context<Self>) {
        let result = self
            .home
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Home profiles have no configuration directory."))
            .and_then(|path| self.home.profiles.write(path));
        if let Err(error) = result {
            self.set_error(format!("{error:#}"), cx);
        }
        cx.notify();
    }
}

fn adjusted(widget: Widget, columns: u8, (horizontal, vertical): (i32, i32)) -> Widget {
    Widget {
        columns: shifted_span(widget.columns, horizontal, 1, columns),
        rows: shifted_span(widget.rows, vertical, MIN_ROWS, VISIBLE_ROWS),
        ..widget
    }
}

fn scroll_horizontally(handle: &ScrollHandle, step: Pixels, limit: Pixels, window: &mut Window) {
    let offset = handle.offset().x;
    let next = (offset + step).clamp(-limit, px(0.));
    if next != offset {
        handle.set_offset(point(next, px(0.)));
        window.refresh();
    }
}

fn rail_control(
    theme: Theme,
    id: &'static str,
    description: &'static str,
    symbol: Icon,
    active: bool,
    action: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    theme.bind(
        theme
            .button(id, active.then_some(TEAL))
            .size(r(RAIL_BUTTON_SIZE))
            .px_0()
            .child(icon(
                symbol,
                theme.color(if active { TEAL } else { TEXT }),
                17.,
            ))
            .tooltip(move |_, cx| cx.new(|_| Tooltip(description.into())).into()),
        action,
    )
}

fn tile_button(
    theme: Theme,
    id: impl Into<gpui::ElementId>,
    description: &'static str,
    symbol: Icon,
    color: Color,
) -> gpui::Stateful<gpui::Div> {
    theme
        .button(id, None)
        .w(r(27.))
        .h(r(WIDGET_CONTROL_HEIGHT))
        .px_0()
        .child(icon(symbol, theme.color(color), 14.))
        .tooltip(move |_, cx| cx.new(|_| Tooltip(description.into())).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_stacked_two_row_widgets_fill_the_visible_grid() {
        for height in [276., 349., 600.] {
            let grid = Grid::new(1400., height);
            let stacked = 2. * grid.span_height(2) + GAP;
            assert!((stacked - height).abs() < 0.001);
            assert!(grid.span_height(5) > height);
        }
    }

    #[test]
    fn cells_preserve_the_three_row_aspect_ratio_on_an_ultrawide_display() {
        let (width, height) = (1493., 349.);
        let grid = Grid::new(width, height);
        let old_width = (width - 3. * GAP) / 4.;
        let old_height = (height - 2. * GAP) / 3.;
        assert_eq!(grid.columns, 5);
        assert!((grid.width / grid.height - old_width / old_height).abs() < 0.001);
        assert!(grid.span_width(usize::from(grid.columns)) <= width);
        assert!(grid.span_width(usize::from(grid.columns) + 1) > width);
    }

    #[test]
    fn columns_keep_space_for_resize_controls_on_narrow_windows() {
        let grid = Grid::new(900., 276.);
        assert_eq!(grid.columns, 3);
        assert_eq!(grid.width, MIN_COLUMN_WIDTH);
    }
}
