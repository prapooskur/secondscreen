use gpui::{Context, IntoElement, prelude::*};

use super::api::Action;
use crate::ui::{Dashboard, Tooltip, icons::Icon, theme::*};

impl Dashboard {
    pub(in crate::ui) fn home_assistant_widget(
        &self,
        widget: u32,
        height: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let integration = &self.home_assistant;
        let body = col()
            .id(("ha-widget", widget as usize))
            .flex_1()
            .overflow_y_scroll()
            .gap(r(7.));
        let Some(binding) = self.home.home_assistant_binding(widget).filter(|binding| {
            self.preferences
                .home_assistant
                .as_ref()
                .is_some_and(|saved| saved.url == binding.instance)
        }) else {
            let choose = self.choose_home_assistant(widget, "Choose entity", cx);
            let content = if height < 145. {
                row().flex_1().justify_center().child(choose)
            } else {
                theme.empty(Icon::Home, BLUE, "Home Assistant", choose)
            };
            return body.child(content);
        };
        let device = integration
            .devices
            .iter()
            .flatten()
            .find(|device| device.id == binding.entity);
        let Some(device) = device else {
            let detail = integration.connection_error.as_deref().unwrap_or_else(|| {
                if integration.devices.is_some() {
                    "Entity no longer found."
                } else {
                    "Connecting to Home Assistant…"
                }
            });
            return body
                .child(theme.label(binding.entity.clone()).truncate())
                .child(theme.quiet(detail.to_owned()))
                .when(integration.devices.is_some(), |body| {
                    body.child(row().child(self.choose_home_assistant(widget, "Choose entity", cx)))
                });
        };

        let pending = integration.pending.contains(&device.id);
        let enabled = device.available() && integration.connection_error.is_none() && !pending;
        let name = device.name().to_owned();
        let state = if pending {
            "Sending…".into()
        } else {
            device.state_label()
        };
        let message = integration
            .connection_error
            .as_ref()
            .or_else(|| integration.action_errors.get(&device.id));
        let actions = device.actions();
        let adjustment = device.adjustment();
        let no_controls = actions.is_empty() && adjustment.is_none();
        body.child(
            col()
                .gap(r(2.))
                .flex_none()
                .child(
                    theme
                        .title(name.clone())
                        .text_size(r(18.))
                        .id("ha-entity-title")
                        .truncate()
                        .tooltip(move |_, cx| cx.new(|_| Tooltip(name.clone().into())).into()),
                )
                .child(theme.quiet(state)),
        )
        .when(!actions.is_empty(), |body| {
            body.child(row().flex_none().flex_wrap().gap(r(5.)).children(
                actions.into_iter().enumerate().map(|(index, action)| {
                    self.home_assistant_action(
                        ("ha-action", index),
                        &device.id,
                        action,
                        enabled,
                        cx,
                    )
                }),
            ))
        })
        .children(adjustment.map(|adjustment| {
            let step = |id, label, action| match action {
                Some(action) => self.home_assistant_action(id, &device.id, action, enabled, cx),
                None => disabled(
                    theme
                        .button(id, None)
                        .h(r(36.))
                        .min_w(r(38.))
                        .px(r(5.))
                        .child(label),
                ),
            };
            row()
                .flex_none()
                .flex_wrap()
                .gap(r(5.))
                .child(theme.quiet(adjustment.label).flex_1())
                .child(step("ha-decrease", "−", adjustment.down))
                .child(step("ha-increase", "+", adjustment.up))
        }))
        .children(message.map(|message| theme.quiet(message.clone()).text_color(theme.color(RED))))
        .when(no_controls && enabled, |body| {
            body.child(theme.quiet("No controls available."))
        })
    }

    fn home_assistant_action(
        &self,
        element: impl Into<gpui::ElementId>,
        id: &str,
        action: Action,
        enabled: bool,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::current(cx);
        let button = theme
            .button(element, Some(BLUE))
            .h(r(36.))
            .min_w(r(38.))
            .px(r(8.))
            .child(action.label.clone());
        if !enabled {
            return disabled(button);
        }
        let view = cx.entity();
        let id = id.to_owned();
        theme.bind(button, move |_, cx| {
            view.update(cx, |this, cx| {
                this.send_home_assistant(id.clone(), action.clone(), cx)
            });
        })
    }
}
