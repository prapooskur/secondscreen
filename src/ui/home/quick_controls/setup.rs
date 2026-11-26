use gpui::{Context, Div, Stateful, Styled, Window, prelude::*};

use super::QuickAction;
use crate::{
    platform::{desktop_apps, in_flatpak},
    ui::{
        Dashboard, Tooltip,
        home::Kind,
        icons::{Icon, icon},
        settings::QuickAppPicker,
        text_input::TextInput,
        theme::*,
    },
};

#[derive(Clone)]
enum Change {
    Add(QuickAction),
    Move(QuickAction, isize),
    Remove(QuickAction),
    Reset,
}

impl Dashboard {
    pub(in crate::ui::home) fn quick_controls_setup_button(
        &self,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let view = cx.entity();
        theme.bind(
            theme
                .button("quick-controls-setup", None)
                .h(r(26.))
                .px(r(7.))
                .child("Setup"),
            move |window, cx| {
                view.update(cx, |this, cx| this.open_quick_controls_setup(window, cx))
            },
        )
    }

    pub(in crate::ui) fn quick_controls_setup(&self, cx: &Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let layout = self.home.layout();
        let selected = layout.quick_actions();
        if let Some(picker) = &self.settings.quick_app_picker {
            return self.quick_app_picker(picker, selected, cx);
        }
        let available = [QuickAction::DEFAULT, QuickAction::ADDITIONAL]
            .into_iter()
            .flatten()
            .filter(|action| !selected.contains(action));
        let view = cx.entity();
        let reset = theme
            .button("quick-controls-reset", None)
            .h(r(28.))
            .px(r(8.))
            .child("Reset to defaults");
        let reset = if layout.quick_controls.is_some() {
            theme.bind(reset, move |_, cx| {
                view.update(cx, |this, cx| this.edit_quick_controls(Change::Reset, cx));
            })
        } else {
            disabled(reset)
        };

        let assigned = col()
            .id("assigned-quick-actions")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(4.))
            .when(selected.is_empty(), |list| {
                list.child(theme.quiet("No actions on the tile."))
            })
            .children(selected.iter().enumerate().map(|(index, action)| {
                let earlier = Change::Move(action.clone(), -1);
                let later = Change::Move(action.clone(), 1);
                action_row(theme, action, Some(index + 1))
                    .child(edit_button(theme, earlier, index > 0, cx))
                    .child(edit_button(theme, later, index + 1 < selected.len(), cx))
                    .child(edit_button(theme, Change::Remove(action.clone()), true, cx))
            }));

        let launch_app = theme
            .button("choose-quick-app", None)
            .h(r(32.))
            .px(r(6.))
            .justify_start()
            .child(icon(Icon::Grid, theme.color(TEAL), 15.))
            .child("Launch app…");
        let launch_app = if in_flatpak() {
            disabled(launch_app)
        } else {
            let view = cx.entity();
            theme.bind(launch_app, move |_, cx| {
                view.update(cx, |this, cx| this.open_quick_app_picker(cx));
            })
        };
        let catalog = col()
            .id("available-quick-actions")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(4.))
            .child(launch_app)
            .when(in_flatpak(), |list| {
                list.child(theme.quiet(desktop_apps::UNAVAILABLE))
            })
            .children(available.map(|action| {
                action_row(theme, action, None).child(edit_button(
                    theme,
                    Change::Add(action.clone()),
                    true,
                    cx,
                ))
            }));

        col()
            .flex_1()
            .gap(r(10.))
            .child(
                row()
                    .flex_none()
                    .justify_between()
                    .gap(r(8.))
                    .child(theme.quiet("Buttons fill each row from left to right."))
                    .child(reset),
            )
            .child(
                split()
                    .flex_1()
                    .min_h_0()
                    .gap(r(10.))
                    .child(setup_column(theme, "On the tile").child(assigned))
                    .child(setup_column(theme, "Available actions").child(catalog)),
            )
    }

    fn open_quick_app_picker(&mut self, cx: &mut Context<Self>) {
        let search = cx.new(|cx| TextInput::with_placeholder("Search apps", cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        let load = cx.spawn(async move |this, cx| {
            let apps = blocking::unblock(desktop_apps::installed).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(picker) = &mut this.settings.quick_app_picker {
                    picker.apps = Some(apps);
                    cx.notify();
                }
            });
        });
        self.settings.quick_app_picker = Some(QuickAppPicker {
            search,
            apps: None,
            _load: load,
        });
        cx.notify();
    }

    fn quick_app_picker(
        &self,
        picker: &QuickAppPicker,
        selected: &[QuickAction],
        cx: &Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let view = cx.entity();
        let back = theme.bind(
            theme
                .button("back-from-quick-apps", None)
                .h(r(28.))
                .child("Back"),
            move |window, cx| {
                view.update(cx, |this, cx| this.close_quick_app_picker(window, cx));
            },
        );
        let search = picker.search.read(cx).value.trim().to_lowercase();
        let matches = picker
            .apps
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|app| {
                !selected.iter().any(|action| action.key() == app.id)
                    && (app.name.to_lowercase().contains(&search)
                        || app.id.to_lowercase().contains(&search))
            });
        let empty = match &picker.apps {
            None => "Looking for installed apps…",
            Some(_) if search.is_empty() => "No apps available to add.",
            Some(_) => "No apps match your search.",
        };

        col()
            .flex_1()
            .gap(r(8.))
            .child(
                row()
                    .justify_between()
                    .child(theme.label("Choose an app to add"))
                    .child(back),
            )
            .child(picker.search.clone())
            .child(
                col()
                    .id("quick-apps")
                    .flex_1()
                    .overflow_y_scroll()
                    .gap(r(4.))
                    .when(matches.clone().next().is_none(), |list| {
                        list.child(theme.quiet(empty))
                    })
                    .children(matches.map(|app| {
                        let view = cx.entity();
                        let app = app.clone();
                        theme.bind(
                            theme
                                .button(format!("add-app-{}", app.id), None)
                                .h(r(42.))
                                .child(
                                    col()
                                        .flex_1()
                                        .child(theme.label(app.name.clone()).truncate())
                                        .child(theme.quiet(app.id.clone()).truncate()),
                                )
                                .child(icon(Icon::Plus, theme.color(TEAL), 15.)),
                            move |window, cx| {
                                view.update(cx, |this, cx| {
                                    let action = QuickAction::LaunchApp(app.clone());
                                    if this.edit_quick_controls(Change::Add(action), cx) {
                                        this.close_quick_app_picker(window, cx);
                                    }
                                });
                            },
                        )
                    })),
            )
    }

    fn close_quick_app_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.quick_app_picker = None;
        self.focus_overlay(window, cx);
        cx.notify();
    }

    fn edit_quick_controls(&mut self, change: Change, cx: &mut Context<Self>) -> bool {
        if !self.settings.quick_controls_setup || !self.home.contains(Kind::QuickControls) {
            return false;
        }
        let layout = self.home.layout_mut();
        let mut actions = layout.quick_actions().to_vec();
        match change {
            Change::Add(action) if !actions.iter().any(|item| item.key() == action.key()) => {
                actions.push(action);
            }
            Change::Add(_) => return false,
            Change::Remove(action) => actions.retain(|item| *item != action),
            Change::Move(action, step) => {
                let Some(index) = actions.iter().position(|item| *item == action) else {
                    return false;
                };
                let Some(destination) = index
                    .checked_add_signed(step)
                    .filter(|&i| i < actions.len())
                else {
                    return false;
                };
                actions.swap(index, destination);
            }
            Change::Reset => actions = QuickAction::DEFAULT.to_vec(),
        }

        let update = (actions != QuickAction::DEFAULT).then_some(actions);
        if update == layout.quick_controls {
            return false;
        }
        let previous = std::mem::replace(&mut layout.quick_controls, update);
        if self.save_home(cx) {
            return true;
        }
        self.home.layout_mut().quick_controls = previous;
        false
    }
}

fn setup_column(theme: Theme, heading: &'static str) -> Div {
    col()
        .flex_1()
        .gap(r(7.))
        .child(theme.label(heading).text_color(theme.color(INK)))
}

fn action_row(theme: Theme, action: &QuickAction, position: Option<usize>) -> Stateful<Div> {
    let label = action.label();
    let (_, symbol) = action.presentation();
    row()
        .id(format!("setup-action-{}", action.key()))
        .flex_none()
        .h(r(32.))
        .px(r(5.))
        .gap(r(4.))
        .rounded(r(6.))
        .bg(theme.color(HEADER))
        .children(position.map(|position| {
            mono(position.to_string())
                .w(r(18.))
                .text_center()
                .text_size(r(11.))
                .text_color(theme.color(MUTED))
        }))
        .child(action.icon(symbol, theme, TEXT, 15.))
        .child(
            theme
                .label(label.clone())
                .flex_1()
                .min_w_0()
                .text_size(r(12.))
                .truncate(),
        )
        .tooltip(move |_, cx| cx.new(|_| Tooltip(label.clone())).into())
}

fn edit_button(
    theme: Theme,
    change: Change,
    enabled: bool,
    cx: &Context<Dashboard>,
) -> Stateful<Div> {
    let (label, symbol, color) = match &change {
        Change::Add(_) => ("Add to grid", Icon::Plus, TEAL),
        Change::Move(_, -1) => ("Move earlier", Icon::Up, TEXT),
        Change::Move(_, _) => ("Move later", Icon::Down, TEXT),
        Change::Remove(_) => ("Remove from grid", Icon::Close, RED),
        Change::Reset => unreachable!(),
    };
    let button = theme
        .button(label, None)
        .size(r(23.))
        .px_0()
        .child(icon(symbol, theme.color(color), 13.))
        .tooltip(move |_, cx| cx.new(|_| Tooltip(label.into())).into());
    if !enabled {
        return disabled(button);
    }
    let view = cx.entity();
    theme.bind(button, move |_, cx| {
        view.update(cx, |this, cx| this.edit_quick_controls(change.clone(), cx));
    })
}
