use gpui::{Context, Entity, IntoElement, PathPromptOptions, Task, prelude::*};

use super::{
    Dashboard,
    home::{Badge, import_profile_image},
    overlay::Overlay,
    text_input::{TextInput, field},
    theme::*,
};

#[derive(Default)]
pub(super) struct ProfileEditor {
    label: Option<Entity<TextInput>>,
    picker: Option<Task<()>>,
}

impl Dashboard {
    pub(super) fn profile_editor_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (index, profile) = self.home.active_profile();
        let initial_label = profile.label(index);
        let input = self
            .profile_editor
            .label
            .get_or_insert_with(|| {
                cx.new(|cx| {
                    let mut input = TextInput::with_placeholder("Letter or number", cx);
                    input.set_value(initial_label, cx);
                    input
                })
            })
            .clone();
        let view = cx.entity();
        let save = theme.bind(
            theme
                .button("save-profile-label", Some(TEAL))
                .child("Save label"),
            {
                let input = input.clone();
                let view = view.clone();
                move |_, cx| {
                    let mut chars = input.read(cx).value.trim().chars();
                    let badge = match (chars.next(), chars.next()) {
                        (Some(label), None) if label.is_alphanumeric() => {
                            Ok(Some(Badge::Label(label)))
                        }
                        _ => Err(anyhow::anyhow!("Enter a single letter or number.")),
                    };
                    view.update(cx, |this, cx| {
                        this.profile_editor.picker = None;
                        this.save_profile_badge(index, badge, cx);
                    });
                }
            },
        );
        let choose = theme.bind(
            theme
                .button("choose-profile-image", None)
                .child("Choose SVG or PNG"),
            {
                let view = view.clone();
                move |_, cx| view.update(cx, |this, cx| this.choose_profile_image(index, cx))
            },
        );
        let reset = theme.bind(
            theme
                .button("reset-profile-badge", None)
                .child("Use default"),
            move |_, cx| {
                view.update(cx, |this, cx| {
                    this.profile_editor.picker = None;
                    this.save_profile_badge(index, Ok(None), cx);
                });
            },
        );
        let title = row()
            .gap(r(10.))
            .child(
                row()
                    .size(r(36.))
                    .flex_none()
                    .justify_center()
                    .rounded(r(8.))
                    .bg(theme.color(TEAL).opacity(0.13))
                    .text_color(theme.color(TEAL))
                    .child(profile.button_content(index, 26.)),
            )
            .child(theme.title(format!("Edit profile {}", index + 1)));
        self.overlay_panel(Overlay::Profile, title, cx).child(
            col()
                .id("profile-editor-content")
                .flex_1()
                .overflow_y_scroll()
                .child(
                    col()
                        .flex_none()
                        .gap(r(10.))
                        .child(field(theme, "Letter or number", input))
                        .child(
                            row()
                                .gap(r(6.))
                                .flex_wrap()
                                .child(save)
                                .child(choose)
                                .child(reset),
                        ),
                ),
        )
    }

    fn choose_profile_image(&mut self, index: usize, cx: &mut Context<Self>) {
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use as profile image".into()),
        });
        self.profile_editor.picker = Some(cx.spawn(async move |this, cx| {
            let badge = match result.await {
                Ok(Ok(Some(paths))) => {
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    blocking::unblock(move || import_profile_image(&path))
                        .await
                        .map(Some)
                }
                Ok(Err(error)) => Err(error),
                _ => return,
            };
            let _ = this.update(cx, |this, cx| this.save_profile_badge(index, badge, cx));
        }));
    }

    fn save_profile_badge(
        &mut self,
        index: usize,
        badge: anyhow::Result<Option<Badge>>,
        cx: &mut Context<Self>,
    ) {
        match badge.and_then(|badge| self.set_profile_badge(index, badge, cx)) {
            Ok(()) => {
                if let Some(input) = &self.profile_editor.label {
                    let (index, profile) = self.home.active_profile();
                    input.update(cx, |input, cx| input.set_value(profile.label(index), cx));
                }
            }
            Err(error) => {
                self.set_error(format!("{error:#}"), cx);
                cx.notify();
            }
        }
    }
}
