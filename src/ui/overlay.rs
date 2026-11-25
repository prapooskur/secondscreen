use gpui::{Context, Div, FocusHandle, IntoElement, Stateful, Window, div, prelude::*};

use super::{
    CONTENT_X, CONTENT_Y, Dashboard, Tooltip,
    icons::{Icon, icon},
    theme::*,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Overlay {
    Settings,
    Profile,
}

pub(super) struct OverlayView {
    pub(super) open: Option<Overlay>,
    pub(super) focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
}

impl OverlayView {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: None,
            focus,
            previous_focus: None,
        }
    }
}

impl Dashboard {
    pub(super) fn show_overlay(
        &mut self,
        overlay: Overlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.overlay.open != Some(overlay) {
            if let Some(previous) = self.overlay.open.replace(overlay) {
                self.reset_overlay(previous);
            } else {
                self.overlay.previous_focus = window.focused(cx);
                self.gesture.borrow_mut().take();
            }
            self.focus_overlay(window, cx);
        }
        cx.notify();
    }

    pub(super) fn focus_overlay(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.overlay.focus, cx);
    }

    pub(super) fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(overlay) = self.overlay.open.take() else {
            return;
        };
        self.reset_overlay(overlay);
        let previous = self
            .overlay
            .previous_focus
            .take()
            .filter(|focus| self.focus.contains(focus, window))
            .unwrap_or_else(|| self.focus.clone());
        window.focus(&previous, cx);
        cx.notify();
    }

    fn reset_overlay(&mut self, overlay: Overlay) {
        match overlay {
            Overlay::Settings => self.reset_settings(),
            Overlay::Profile => self.profile_editor = Default::default(),
        }
    }

    pub(super) fn render_overlay(
        &mut self,
        overlay: Overlay,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = match overlay {
            Overlay::Settings => self.settings_panel(cx).into_any_element(),
            Overlay::Profile => self.profile_editor_panel(cx).into_any_element(),
        };
        split()
            .absolute()
            .inset_0()
            .when(overlay == Overlay::Settings, |overlay| {
                overlay.justify_end()
            })
            .px(r(CONTENT_X))
            .py(r(CONTENT_Y))
            .child(
                div()
                    .id("overlay-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(theme.color(SCRIM).opacity(0.52))
                    .occlude()
                    .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx))),
            )
            .child(panel)
    }

    pub(super) fn overlay_panel(
        &self,
        overlay: Overlay,
        title: impl IntoElement,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let view = cx.entity();
        let (id, width, tooltip) = match overlay {
            Overlay::Settings => ("settings-panel", 540., "Close settings"),
            Overlay::Profile => ("profile-editor-panel", 380., "Close profile editor"),
        };
        theme
            .panel()
            .id(id)
            .w(r(width))
            .max_w_full()
            .h_full()
            .flex_none()
            .occlude()
            .track_focus(&self.overlay.focus)
            .tab_index(0)
            .tab_stop(false)
            .tab_group()
            .overflow_hidden()
            .p(r(12.))
            .gap(r(9.))
            .child(
                row().justify_between().child(title).child(
                    theme.bind(
                        theme
                            .button("dismiss-overlay", None)
                            .size(r(28.))
                            .px_0()
                            .child(icon(Icon::Close, theme.color(TEXT), 14.))
                            .tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip.into())).into()),
                        move |window, cx| {
                            view.update(cx, |this, cx| this.close_overlay(window, cx))
                        },
                    ),
                ),
            )
    }
}
