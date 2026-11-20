mod layout;
mod widgets;

use std::{path::PathBuf, time::Instant};

use gpui::{
    Context, DispatchPhase, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    canvas, div, prelude::*,
};

use self::layout::{
    Kind, Layout, MAX_COLUMNS, MAX_ROWS, MIN_ROWS, Placement, Widget, shifted_span,
};
use super::{
    Dashboard, Page, Tooltip,
    icons::{Icon, icon},
    theme::*,
};

const GAP: f32 = 12.;
const HEADING_HEIGHT: f32 = 37.;
const EDIT_HEADING_HEIGHT: f32 = 60.;

pub(super) struct Home {
    layout: Layout,
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
    fn new(width: f32, height: f32) -> Self {
        let width = width.max(1.);
        let columns = (((width + GAP) / (232. + GAP)).floor() as u8).clamp(1, MAX_COLUMNS);
        Self {
            columns,
            width: (width - GAP * f32::from(columns - 1)) / f32::from(columns),
            height: ((height - 2. * GAP) / 3.).clamp(80., 118.),
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
            path,
            editing: false,
            catalog: false,
            resizing: None,
        }
    }

    pub(super) fn includes(&self, page: Page) -> bool {
        self.layout
            .widgets
            .iter()
            .any(|widget| widget.kind.page() == page)
    }

    pub(super) fn edit(&mut self) {
        self.editing = true;
    }

    pub(super) fn leave(&mut self) {
        self.editing = false;
        self.catalog = false;
        self.resizing = None;
    }

    pub(super) fn escape(&mut self) -> bool {
        if self.resizing.take().is_some() {
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
                .filter(|drag| drag.original.kind == widget.kind)
                .map_or(*widget, |drag| drag.preview)
        })
    }

    fn resize(&mut self, next: Widget) -> bool {
        let Some(widget) = self
            .layout
            .widgets
            .iter_mut()
            .find(|widget| widget.kind == next.kind)
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
        cx: &Context<Self>,
    ) -> impl IntoElement {
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
                    .overflow_y_scroll()
                    .child(
                        div()
                            .relative()
                            .w_full()
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
            .child(self.home_heading(heading_height, cx))
            .child(content)
            .when(self.home.resizing.is_some(), |it| {
                it.child(self.track_resize(cx))
            })
    }

    fn home_heading(&self, height: f32, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let editing = self.home.editing;
        let catalog = self.home.catalog;
        let can_add = self.home.layout.widgets.len() < Kind::ALL.len();
        let toggle = bind(
            button("edit-home", editing.then_some(TEAL))
                .child(icon(
                    if editing { Icon::Check } else { Icon::Edit },
                    c(if editing { TEAL } else { TEXT }),
                    15.,
                ))
                .child(if editing { "Done" } else { "Edit layout" }),
            move |_, cx| {
                view.update(cx, |this, cx| {
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
                    .gap(r(3.))
                    .child(title(if catalog { "Add widgets" } else { "Home" }))
                    .when(editing && !catalog, |it| {
                        it.child(quiet("Drag a corner or use the size controls.").truncate())
                    }),
            )
            .child(
                row()
                    .flex_none()
                    .gap(r(8.))
                    .when(editing && (catalog || can_add), |it| {
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
                    .child(toggle),
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
                move |_, cx| {
                    view.update(cx, |this, cx| {
                        this.home.editing = true;
                        this.home.catalog = true;
                        cx.notify();
                    });
                },
            ))
    }

    fn widget_catalog(&self, grid: Grid, cx: &Context<Self>) -> impl IntoElement {
        let available = Kind::ALL
            .into_iter()
            .filter(|kind| {
                !self
                    .home
                    .layout
                    .widgets
                    .iter()
                    .any(|widget| widget.kind == *kind)
            })
            .collect::<Vec<_>>();

        col()
            .id("widget-catalog-scroll")
            .flex_1()
            .overflow_y_scroll()
            .when(available.is_empty(), |it| {
                it.child(quiet("All widgets are on Home.").p(r(6.)))
            })
            .child(
                split()
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
                            move |_, cx| {
                                view.update(cx, |this, cx| {
                                    if !this
                                        .home
                                        .layout
                                        .widgets
                                        .iter()
                                        .any(|widget| widget.kind == kind)
                                    {
                                        this.home.layout.widgets.push(kind.widget());
                                        this.save_home(cx);
                                    }
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
        let view = cx.entity();
        let tools = if editing {
            row()
                .gap(r(4.))
                .child(self.widget_move(kind, -1, index > 0, cx))
                .child(self.widget_move(kind, 1, index + 1 < count, cx))
                .child(bind(
                    tile_button(
                        ("remove-widget", kind as usize),
                        "Remove widget",
                        Icon::Close,
                        RED,
                    ),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            this.home.resizing = None;
                            this.home
                                .layout
                                .widgets
                                .retain(|widget| widget.kind != kind);
                            this.save_home(cx);
                        })
                    },
                ))
                .into_any_element()
        } else {
            bind(
                tile_button(
                    ("widget-details", kind as usize),
                    kind.page().label(),
                    Icon::Chevron,
                    MUTED,
                ),
                move |window, cx| view.update(cx, |this, cx| this.select(kind.page(), window, cx)),
            )
            .into_any_element()
        };

        panel()
            .absolute()
            .top(r(tile.y as f32 * (grid.height + GAP)))
            .left(r(tile.x as f32 * (grid.width + GAP)))
            .w(r(grid.span_width(tile.columns)))
            .h(r(grid.span_height(usize::from(tile.widget.rows))))
            .p(r(13.))
            .gap(r(11.))
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
                            .child(icon_tile(kind.icon(), kind.accent(), 29.))
                            .child(label(kind.name()).text_color(c(INK)).truncate()),
                    )
                    .child(tools),
            )
            .child(self.widget_content(tile, cx))
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
        kind: Kind,
        direction: isize,
        enabled: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let (name, symbol) = if direction < 0 {
            ("Move earlier", Icon::Left)
        } else {
            ("Move later", Icon::Chevron)
        };
        let control = tile_button((name, kind as usize), name, symbol, TEXT);
        if !enabled {
            return disabled(control);
        }
        let view = cx.entity();
        bind(control, move |_, cx| {
            view.update(cx, |this, cx| {
                this.home.resizing = None;
                let widgets = &mut this.home.layout.widgets;
                if let Some(index) = widgets.iter().position(|widget| widget.kind == kind)
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
            ("resize-widget", widget.kind as usize),
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
                this.adjust_widget(widget.kind, grid.columns, (horizontal, vertical), cx);
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
            let control = tile_button(
                (description, widget.kind as usize),
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
            bind(control, move |_, cx| {
                view.update(cx, |this, cx| {
                    this.adjust_widget(widget.kind, columns, delta, cx)
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

    fn adjust_widget(
        &mut self,
        kind: Kind,
        columns: u8,
        delta: (i32, i32),
        cx: &mut Context<Self>,
    ) {
        if let Some(current) = self
            .home
            .layout
            .widgets
            .iter()
            .find(|item| item.kind == kind)
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
        .h(r(28.))
        .px_0()
        .child(icon(symbol, c(color), 14.))
        .tooltip(move |_, cx| cx.new(|_| Tooltip(description.into())).into())
}
