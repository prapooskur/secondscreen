mod layout;
mod options;
mod performance;
mod widgets;

use std::{path::PathBuf, time::Instant};

use gpui::{
    Context, DispatchPhase, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    canvas, div, prelude::*,
};

pub(super) use self::layout::Kind;
use self::layout::{Layout, MAX_COLUMNS, MAX_ROWS, MIN_ROWS, Placement, Widget, shifted_span};
use super::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    theme::*,
    window_control,
};

const GAP: f32 = 12.;
const VISIBLE_ROWS: u8 = 4;
const REFERENCE_ROWS: u8 = 3;
const REFERENCE_COLUMNS: u8 = 4;
const MIN_COLUMN_WIDTH: f32 = 232.;
const HEADING_HEIGHT: f32 = 37.;
const EDIT_HEADING_HEIGHT: f32 = 60.;
const WIDGET_HEADER_HEIGHT: f32 = 29.;
const RESIZE_CONTROL_HEIGHT: f32 = 28.;

pub(super) struct Home {
    layout: Layout,
    sensor_editor: Option<(u32, usize)>,
    path: Option<PathBuf>,
    editing: bool,
    catalog: bool,
    resizing: Option<Resize>,
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

    fn span_width(self, columns: u8) -> f32 {
        f32::from(columns) * (self.width + GAP) - GAP
    }

    fn span_height(self, rows: usize) -> f32 {
        rows as f32 * (self.height + GAP) - GAP
    }
}

impl Home {
    pub(super) fn load() -> Self {
        let path = crate::config::path("home.json");
        let layout = path
            .as_deref()
            .map(Layout::read)
            .transpose()
            .unwrap_or_else(|error| {
                log::warn!("Could not load the Home layout: {error:#}");
                None
            })
            .unwrap_or_default();
        Self {
            layout,
            sensor_editor: None,
            path,
            editing: false,
            catalog: false,
            resizing: None,
        }
    }

    pub(super) fn contains(&self, kind: Kind) -> bool {
        self.layout.widgets.iter().any(|widget| widget.kind == kind)
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
        self.layout.widgets.iter().map(|widget| {
            self.resizing
                .as_ref()
                .filter(|drag| drag.original.id == widget.id)
                .map_or(*widget, |drag| drag.preview)
        })
    }

    fn resize(&mut self, next: Widget) -> bool {
        let Some(widget) = self
            .layout
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
        let heading_height = if self.home.editing && !self.home.catalog {
            EDIT_HEADING_HEIGHT
        } else {
            HEADING_HEIGHT
        };
        let grid = Grid::new(width, height - heading_height - GAP);
        let content = if self.home.catalog {
            self.widget_catalog(grid, cx).into_any_element()
        } else {
            let placements = layout::place(self.home.widgets(), grid.columns);
            if placements.is_empty() {
                self.empty_home(cx).into_any_element()
            } else {
                let rows = placements
                    .iter()
                    .map(|tile| tile.y + usize::from(tile.widget.rows))
                    .max()
                    .unwrap_or_default();
                let count = placements.len();
                col()
                    .id("home-widget-scroll")
                    .flex_1()
                    .items_center()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .relative()
                            .w(r(grid.span_width(grid.columns)))
                            .h(r(grid.span_height(rows)))
                            .flex_none()
                            .children(placements.into_iter().enumerate().map(|(index, tile)| {
                                self.widget_tile(tile, index, count, grid, scale, cx)
                            })),
                    )
                    .into_any_element()
            }
        };

        col()
            .relative()
            .flex_1()
            .gap(r(GAP))
            .child(self.home_heading(heading_height, fullscreen, cx))
            .child(content)
            .when(self.home.resizing.is_some(), |it| {
                it.child(self.track_resize(cx))
            })
    }

    fn home_heading(&self, height: f32, fullscreen: bool, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let editing = self.home.editing;
        let catalog = self.home.catalog;
        let toggle = bind(
            button("edit-home", editing.then_some(TEAL))
                .child(icon(
                    if editing { Icon::Check } else { Icon::Edit },
                    c(if editing { TEAL } else { TEXT }),
                    15.,
                ))
                .child(if editing { "Done" } else { "Edit layout" }),
            move |window, cx| {
                view.update(cx, |this, cx| {
                    this.close_settings(window, cx);
                    if this.home.editing {
                        this.home.leave();
                    } else {
                        this.home.editing = true;
                    }
                    cx.notify();
                });
            },
        );

        row()
            .h(r(height))
            .flex_none()
            .justify_between()
            .gap(r(12.))
            .child(
                col()
                    .flex_1()
                    .justify_center()
                    .h_full()
                    .gap(r(3.))
                    .when(!fullscreen, |it| {
                        it.cursor_move()
                            .on_mouse_down(MouseButton::Left, |event, window, _| {
                                if event.click_count == 2 {
                                    window.zoom_window();
                                } else {
                                    window.start_window_move();
                                }
                            })
                    })
                    .child(title(if catalog {
                        "Add widgets"
                    } else {
                        "Second Screen"
                    }))
                    .when(editing && !catalog, |it| {
                        it.child(quiet("Drag a corner or use the size controls.").truncate())
                    }),
            )
            .child(
                row()
                    .flex_none()
                    .gap(r(8.))
                    .when(editing, |it| {
                        let view = cx.entity();
                        it.child(bind(
                            button("widget-catalog", None)
                                .child(icon(
                                    if catalog { Icon::Left } else { Icon::Plus },
                                    c(TEXT),
                                    15.,
                                ))
                                .child(if catalog {
                                    "Back to layout"
                                } else {
                                    "Add widget"
                                }),
                            move |_, cx| {
                                view.update(cx, |this, cx| {
                                    this.home.catalog = !this.home.catalog;
                                    this.home.resizing = None;
                                    cx.notify();
                                })
                            },
                        ))
                    })
                    .child(toggle)
                    .child(bind(
                        button("settings", self.settings.open.then_some(TEAL))
                            .child(icon(Icon::Settings, c(TEXT), 15.))
                            .child("Settings"),
                        {
                            let view = cx.entity();
                            move |window, cx| {
                                view.update(cx, |this, cx| {
                                    if this.settings.open {
                                        this.close_settings(window, cx);
                                    } else {
                                        this.open_settings(this.settings.section, window, cx);
                                    }
                                })
                            }
                        },
                    ))
                    .child(window_control(
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
                        30.,
                        {
                            let view = cx.entity();
                            move |window, cx| {
                                view.update(cx, |this, _| this.toggle_fullscreen(window))
                            }
                        },
                    ))
                    .when(!fullscreen, |it| {
                        it.child(window_control(
                            "Minimize",
                            Icon::Minimize,
                            28.,
                            |window, _| window.minimize_window(),
                        ))
                        .child(window_control(
                            "Close",
                            Icon::Close,
                            28.,
                            |window, _| window.remove_window(),
                        ))
                    }),
            )
    }

    fn empty_home(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        panel()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(r(12.))
            .child(icon_tile(Icon::Home, TEAL, 44.))
            .child(title("Your Home is empty"))
            .child(bind(
                button("first-widget", Some(TEAL)).child("Add widget"),
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
                it.child(quiet("All widgets are on Home.").p(r(6.)))
            })
            .child(
                split()
                    .w(r(grid.span_width(grid.columns)))
                    .flex_wrap()
                    .gap(r(GAP))
                    .children(available.into_iter().map(|kind| {
                        let view = cx.entity();
                        bind(
                            button(("add-widget", kind as usize), None)
                                .w(r(grid.width))
                                .h(r(77.))
                                .justify_start()
                                .gap(r(11.))
                                .child(icon_tile(kind.icon(), kind.accent(), 38.))
                                .child(
                                    col()
                                        .flex_1()
                                        .gap(r(3.))
                                        .child(label(kind.name()).text_color(c(INK)).truncate())
                                        .child(quiet(kind.description()).truncate()),
                                )
                                .child(icon(Icon::Plus, c(kind.accent()), 15.)),
                            move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.focus_dashboard(window, cx);
                                    this.home.layout.add(kind);
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
        let kind = tile.widget.kind;
        let editing = self.home.editing;
        let height = grid.span_height(usize::from(tile.widget.rows));
        let (padding, gap) = if height < 155. { (8., 6.) } else { (13., 11.) };
        let mut content_height = height - 2. * (padding + 1.) - WIDGET_HEADER_HEIGHT - gap;
        if editing {
            content_height -= RESIZE_CONTROL_HEIGHT + gap;
        }
        let view = cx.entity();
        let tools = editing.then(|| {
            row()
                .gap(r(4.))
                .child(self.widget_move(tile.widget.id, -1, index > 0, cx))
                .child(self.widget_move(tile.widget.id, 1, index + 1 < count, cx))
                .child(bind(
                    tile_button(
                        ("remove-widget", tile.widget.id as usize),
                        "Remove widget",
                        Icon::Close,
                        RED,
                    ),
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            this.focus_dashboard(window, cx);
                            this.home.resizing = None;
                            this.home
                                .layout
                                .widgets
                                .retain(|widget| widget.id != tile.widget.id);
                            this.home.layout.sensors.remove(&tile.widget.id);
                            this.home.layout.viewers.remove(&tile.widget.id);
                            this.viewers.remove(&tile.widget.id);
                            this.home.sensor_editor = None;
                            this.save_home(cx);
                        })
                    },
                ))
        });

        panel()
            .absolute()
            .top(r(tile.y as f32 * (grid.height + GAP)))
            .left(r(tile.x as f32 * (grid.width + GAP)))
            .w(r(grid.span_width(tile.columns)))
            .h(r(height))
            .p(r(padding))
            .gap(r(gap))
            .overflow_hidden()
            .when(editing, |it| {
                it.border_color(c(kind.accent()).opacity(0.42))
            })
            .child(
                row()
                    .flex_none()
                    .justify_between()
                    .gap(r(6.))
                    .child(
                        row()
                            .flex_1()
                            .gap(r(8.))
                            .child(icon_tile(kind.icon(), kind.accent(), WIDGET_HEADER_HEIGHT))
                            .child(label(kind.name()).text_color(c(INK)).truncate()),
                    )
                    .children(tools)
                    .when(!editing && kind == Kind::Performance, |it| {
                        it.child(self.sensor_button(tile.widget.id, cx))
                    })
                    .when(!editing && kind == Kind::Viewer, |it| {
                        it.children(
                            self.viewers
                                .get(&tile.widget.id)
                                .map(|v| super::viewer::source_button(v.clone(), cx)),
                        )
                    })
                    .when(!editing && kind == Kind::Media, |it| {
                        it.children(self.player_control(cx))
                    }),
            )
            .child(col().flex_1().overflow_hidden().child(self.widget_content(
                tile,
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

    fn widget_move(
        &self,
        id: u32,
        direction: isize,
        enabled: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let (name, symbol) = if direction < 0 {
            ("Move earlier", Icon::Left)
        } else {
            ("Move later", Icon::Chevron)
        };
        let control = tile_button((name, id as usize), name, symbol, TEXT);
        if !enabled {
            return disabled(control);
        }
        let view = cx.entity();
        bind(control, move |_, cx| {
            view.update(cx, |this, cx| {
                this.home.resizing = None;
                let widgets = &mut this.home.layout.widgets;
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
        let pointer = cx.entity();
        let keyboard = pointer.clone();
        tile_button(
            ("resize-widget", widget.id as usize),
            "Drag or use arrow keys to resize",
            Icon::Resize,
            TEAL,
        )
        .cursor_nwse_resize()
        .tab_index(0)
        .focus(|it| it.border_color(c(TEAL)))
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
        let widget = tile.widget;
        let step = |description: &'static str, symbol, horizontal, vertical| {
            let delta = (horizontal, vertical);
            let control = tile_button((description, widget.id as usize), description, symbol, TEXT)
                .w(r(23.))
                .h(r(24.));
            if adjusted(widget, columns, delta) == widget {
                return disabled(control);
            }
            let view = cx.entity();
            bind(control, move |_, cx| {
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
            .child(quiet("×").px(r(2.)))
            .child(step("Shorter", Icon::Minimize, 0, -1))
            .child(number(widget.rows))
            .child(step("Taller", Icon::Plus, 0, 1))
    }

    fn adjust_widget(&mut self, id: u32, columns: u8, delta: (i32, i32), cx: &mut Context<Self>) {
        if let Some(current) = self.home.layout.widgets.iter().find(|item| item.id == id)
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
        for widget in self
            .home
            .layout
            .widgets
            .iter()
            .filter(|w| w.kind == Kind::Viewer)
        {
            let id = widget.id;
            self.viewers.entry(id).or_insert_with(|| {
                let options = self
                    .home
                    .layout
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
        self.home.layout.viewers.insert(id, options);
        self.save_home(cx);
    }

    fn save_home(&mut self, cx: &mut Context<Self>) {
        match &self.home.path {
            Some(path) => {
                if let Err(error) = self.home.layout.write(path) {
                    self.error = Some((Instant::now(), format!("{error:#}")));
                }
            }
            None => {
                self.error = Some((
                    Instant::now(),
                    "Home layout has no configuration directory.".into(),
                ))
            }
        }
        cx.notify();
    }
}

fn adjusted(widget: Widget, columns: u8, (horizontal, vertical): (i32, i32)) -> Widget {
    Widget {
        columns: shifted_span(widget.columns, horizontal, 1, columns),
        rows: shifted_span(widget.rows, vertical, MIN_ROWS, MAX_ROWS),
        ..widget
    }
}

fn tile_button(
    id: impl Into<gpui::ElementId>,
    description: &'static str,
    symbol: Icon,
    color: u32,
) -> gpui::Stateful<gpui::Div> {
    button(id, None)
        .w(r(27.))
        .h(r(RESIZE_CONTROL_HEIGHT))
        .px_0()
        .child(icon(symbol, c(color), 14.))
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
        assert!(grid.span_width(grid.columns) <= width);
        assert!(grid.span_width(grid.columns + 1) > width);
    }

    #[test]
    fn columns_keep_space_for_resize_controls_on_narrow_windows() {
        let grid = Grid::new(900., 276.);
        assert_eq!(grid.columns, 3);
        assert_eq!(grid.width, MIN_COLUMN_WIDTH);
    }
}
