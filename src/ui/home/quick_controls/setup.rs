use gpui::{Context, Div, Stateful, Styled, prelude::*};

use super::QuickAction;
use crate::ui::{
    Dashboard, Tooltip,
    home::Kind,
    icons::{Icon, icon},
    theme::*,
};

#[derive(Clone, Copy)]
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
        let available = [QuickAction::DEFAULT, QuickAction::ADDITIONAL]
            .into_iter()
            .flatten()
            .copied()
            .filter(|action| !selected.contains(action));
        let view = cx.entity();
        let reset = theme
            .button("quick-controls-reset", None)
            .h(r(28.))
            .px(r(8.))
            .child("Reset to defaults");
        let reset = if layout.quick_controls.is_some() {
            theme.bind(reset, move |_, cx| {
                view.update(cx, |this, cx| this.edit_quick_controls(Change::Reset, cx))
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
            .children(selected.iter().enumerate().map(|(index, &action)| {
                let earlier = Change::Move(action, -1);
                let later = Change::Move(action, 1);
                action_row(theme, action, Some(index + 1))
                    .child(edit_button(theme, earlier, index > 0, cx))
                    .child(edit_button(theme, later, index + 1 < selected.len(), cx))
                    .child(edit_button(theme, Change::Remove(action), true, cx))
            }));

        let catalog = col()
            .id("available-quick-actions")
            .flex_1()
            .overflow_y_scroll()
            .gap(r(4.))
            .when(available.clone().next().is_none(), |list| {
                list.child(theme.quiet("All actions are on the tile."))
            })
            .children(available.map(|action| {
                action_row(theme, action, None).child(edit_button(
                    theme,
                    Change::Add(action),
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

    fn edit_quick_controls(&mut self, change: Change, cx: &mut Context<Self>) {
        if !self.settings.quick_controls_setup || !self.home.contains(Kind::QuickControls) {
            return;
        }
        let layout = self.home.layout_mut();
        let mut actions = layout.quick_actions().to_vec();
        match change {
            Change::Add(action) if !actions.contains(&action) => actions.push(action),
            Change::Add(_) => return,
            Change::Remove(action) => actions.retain(|&item| item != action),
            Change::Move(action, step) => {
                let Some(index) = actions.iter().position(|&item| item == action) else {
                    return;
                };
                let Some(destination) = index
                    .checked_add_signed(step)
                    .filter(|&i| i < actions.len())
                else {
                    return;
                };
                actions.swap(index, destination);
            }
            Change::Reset => actions = QuickAction::DEFAULT.to_vec(),
        }

        let update = (actions != QuickAction::DEFAULT).then_some(actions);
        if update != layout.quick_controls {
            let previous = std::mem::replace(&mut layout.quick_controls, update);
            if !self.save_home(cx) {
                self.home.layout_mut().quick_controls = previous;
            }
        }
    }
}

fn setup_column(theme: Theme, heading: &'static str) -> Div {
    col()
        .flex_1()
        .gap(r(7.))
        .child(theme.label(heading).text_color(theme.color(INK)))
}

fn action_row(theme: Theme, action: QuickAction, position: Option<usize>) -> Stateful<Div> {
    let (label, symbol) = action.presentation();
    row()
        .id(("setup-action", action as usize))
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
        .child(icon(symbol, theme.color(TEXT), 15.))
        .child(
            theme
                .label(label)
                .flex_1()
                .min_w_0()
                .text_size(r(12.))
                .truncate(),
        )
        .tooltip(move |_, cx| cx.new(|_| Tooltip(label.into())).into())
}

fn edit_button(
    theme: Theme,
    change: Change,
    enabled: bool,
    cx: &Context<Dashboard>,
) -> Stateful<Div> {
    let (label, symbol, color) = match change {
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
        view.update(cx, |this, cx| this.edit_quick_controls(change, cx))
    })
}
