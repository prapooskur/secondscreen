use std::sync::Arc;

use gpui::{
    App, Context, Div, Entity, IntoElement, PathPromptOptions, RenderImage, Stateful, Task, Window,
    div, prelude::*,
};

use super::{
    Dashboard, Tooltip,
    home::{Badge, MAX_PROFILES, Profile, ProfileDetails, import_profile_image},
    icons::{Icon, ProfileIcon, icon},
    overlay::Overlay,
    text_input::{TextInput, field},
    theme::*,
};

#[derive(Default)]
pub(super) struct ProfileEditor {
    selected: Option<usize>,
    drafts: Vec<Option<Draft>>,
    picker: Option<Task<()>>,
    confirm_delete: bool,
}

struct Draft {
    name: Entity<TextInput>,
    badge: Option<Badge>,
    imported_image: Option<Arc<RenderImage>>,
}

impl ProfileEditor {
    pub(super) fn reset(&mut self, cx: &mut App) {
        for draft in self.drafts.iter_mut().flatten() {
            draft.drop_imported_image(cx);
        }
        *self = Self::default();
    }

    pub(super) fn cancel_delete(&mut self) -> bool {
        std::mem::take(&mut self.confirm_delete)
    }

    fn select(&mut self, index: usize) {
        if self.selected != Some(index) {
            self.picker = None;
            self.confirm_delete = false;
            self.selected = Some(index);
        }
    }
}

impl Draft {
    fn new(profile: &Profile, cx: &mut Context<Dashboard>) -> Self {
        let name = profile.details.name.clone().unwrap_or_default();
        let name = cx.new(|cx| {
            let mut input = TextInput::with_placeholder("Default profile name", cx);
            input.set_value(name, cx);
            input
        });
        cx.observe(&name, |_, _, cx| cx.notify()).detach();
        Self {
            name,
            badge: profile.details.badge.clone(),
            imported_image: None,
        }
    }

    fn name<'a>(&self, cx: &'a App) -> Option<&'a str> {
        let name = self.name.read(cx).value.trim();
        (!name.is_empty()).then_some(name)
    }

    fn changed(&self, profile: &Profile, cx: &App) -> bool {
        self.name(cx) != profile.details.name.as_deref() || self.badge != profile.details.badge
    }

    fn image<'a>(&'a self, profile: &'a Profile) -> Option<&'a Arc<RenderImage>> {
        if matches!(self.badge, Some(Badge::Png(_))) {
            self.imported_image
                .as_ref()
                .or(profile.details.image.as_ref())
        } else {
            None
        }
    }

    fn drop_imported_image(&mut self, cx: &mut App) {
        if let Some(image) = self.imported_image.take() {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
    }
}

impl Dashboard {
    pub(super) fn profile_editor_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (active, _) = self.home.active_profile();
        let count = self.home.profiles().len();
        let editor = &mut self.profile_editor;
        let index = *editor.selected.get_or_insert(active);
        editor.drafts.resize_with(count, || None);
        let profile = &self.home.profiles()[index];
        editor.drafts[index].get_or_insert_with(|| Draft::new(profile, cx));
        let editor = &self.profile_editor;
        let draft = editor.drafts[index].as_ref().unwrap();
        let body = if editor.confirm_delete {
            self.profile_delete_confirmation(theme, index, profile, cx)
                .into_any_element()
        } else {
            self.profile_form(theme, index, active, profile, draft, cx)
                .into_any_element()
        };

        self.overlay_panel(Overlay::Profile, theme.title("Profiles"), cx)
            .child(
                split()
                    .flex_1()
                    .min_h_0()
                    .gap(r(14.))
                    .child(self.profile_list(theme, index, active, cx))
                    .child(body),
            )
    }

    fn profile_list(
        &self,
        theme: Theme,
        selected: usize,
        active: usize,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let profiles = self.home.profiles();
        let add = theme
            .button("new-profile-in-editor", None)
            .child(icon(Icon::Plus, theme.color(TEXT), 15.))
            .child("New profile");
        let add = if profiles.len() == MAX_PROFILES {
            disabled(add).tooltip(move |_, cx| {
                cx.new(|_| Tooltip(format!("Up to {MAX_PROFILES} profiles").into()))
                    .into()
            })
        } else {
            editor_button(theme, add, cx, |this, window, cx| {
                if let Some(index) = this.add_profile(window, cx) {
                    this.profile_editor.select(index);
                }
            })
        };

        col()
            .w(r(158.))
            .max_w(gpui::relative(0.34))
            .flex_none()
            .gap(r(8.))
            .pr(r(12.))
            .border_r_1()
            .border_color(theme.color(STROKE))
            .child(
                col()
                    .id("profile-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .gap(r(5.))
                    .children(profiles.iter().enumerate().map(|(index, profile)| {
                        let name = profile.name(index);
                        let tooltip = if index == active {
                            format!("{name} (active)")
                        } else {
                            name.clone()
                        };
                        editor_button(
                            theme,
                            theme
                                .button(
                                    ("edit-profile-entry", index),
                                    (index == selected).then_some(TEAL),
                                )
                                .h(r(38.))
                                .px(r(7.))
                                .gap(r(6.))
                                .justify_start()
                                .child(
                                    row()
                                        .size(r(24.))
                                        .flex_none()
                                        .justify_center()
                                        .child(profile.button_content(index, theme, 18.)),
                                )
                                .child(col().flex_1().truncate().child(name))
                                .when(index == active, |button| button.child(theme.dot(TEAL)))
                                .tooltip(move |_, cx| {
                                    cx.new(|_| Tooltip(tooltip.clone().into())).into()
                                }),
                            cx,
                            move |this, _, cx| {
                                this.profile_editor.select(index);
                                cx.notify();
                            },
                        )
                    })),
            )
            .child(add)
    }

    fn profile_form(
        &self,
        theme: Theme,
        index: usize,
        active: usize,
        profile: &Profile,
        draft: &Draft,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let status = if active == index {
            theme.chip("Active profile", TEAL).into_any_element()
        } else {
            editor_button(
                theme,
                theme
                    .button("activate-profile", None)
                    .h(r(28.))
                    .child("Set active"),
                cx,
                move |this, window, cx| this.switch_profile(index, window, cx),
            )
            .into_any_element()
        };

        col()
            .flex_1()
            .gap(r(6.))
            .child(
                row()
                    .flex_none()
                    .gap(r(10.))
                    .px(r(9.))
                    .py(r(5.))
                    .rounded(r(9.))
                    .border_1()
                    .border_color(theme.color(STROKE))
                    .bg(theme.color(HEADER))
                    .child(
                        row()
                            .size(r(40.))
                            .flex_none()
                            .justify_center()
                            .rounded(r(8.))
                            .bg(theme.color(TEAL).opacity(0.10))
                            .text_color(theme.color(TEAL))
                            .child(Badge::content(
                                draft.badge.as_ref(),
                                draft.image(profile),
                                draft.name(cx),
                                index,
                                theme,
                                26.,
                            )),
                    )
                    .child(field(theme, "Profile name", draft.name.clone()))
                    .child(status),
            )
            .child(
                col()
                    .id(("profile-form", index))
                    .flex_1()
                    .overflow_y_scroll()
                    .child(self.profile_badge_choices(theme, index, draft, cx)),
            )
            .child(self.profile_form_actions(theme, index, profile, draft, cx))
    }

    fn profile_badge_choices(
        &self,
        theme: Theme,
        index: usize,
        draft: &Draft,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let picking = self.profile_editor.picker.is_some();
        let initial = editor_button(
            theme,
            theme
                .button("profile-initial", draft.badge.is_none().then_some(TEAL))
                .h(r(32.))
                .child("Name initial"),
            cx,
            move |this, _, cx| this.choose_profile_badge(index, None, cx),
        );
        let import = theme
            .button(
                "import-profile-icon",
                matches!(draft.badge, Some(Badge::Png(_))).then_some(TEAL),
            )
            .h(r(32.))
            .child(if picking {
                "Importing…"
            } else {
                "Import SVG or PNG"
            });
        let import = if picking {
            disabled(import)
        } else {
            editor_button(theme, import, cx, move |this, _, cx| {
                this.choose_profile_image(index, cx);
            })
        };
        let presets = ProfileIcon::ALL
            .into_iter()
            .enumerate()
            .map(|(preset_index, preset)| {
                let badge = Badge::Preset(preset);
                editor_button(
                    theme,
                    theme
                        .button(
                            ("profile-preset", preset_index),
                            (draft.badge.as_ref() == Some(&badge)).then_some(TEAL),
                        )
                        .flex_col()
                        .h(r(48.))
                        .px_0()
                        .gap(r(3.))
                        .text_size(r(10.))
                        .child(preset.element(theme, 21.))
                        .child(preset.name()),
                    cx,
                    move |this, _, cx| {
                        this.choose_profile_badge(index, Some(Badge::Preset(preset)), cx)
                    },
                )
            });
        col()
            .flex_none()
            .gap(r(6.))
            .child(
                row()
                    .justify_between()
                    .flex_wrap()
                    .gap(r(5.))
                    .child(theme.label("Profile icon"))
                    .child(row().gap(r(5.)).flex_wrap().child(initial).child(import)),
            )
            .child(div().grid().grid_cols(8).gap(r(5.)).children(presets))
    }

    fn profile_form_actions(
        &self,
        theme: Theme,
        index: usize,
        profile: &Profile,
        draft: &Draft,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let delete = theme
            .button("delete-profile", Some(RED))
            .child("Delete profile");
        let delete = if self.home.profiles().len() == 1 {
            disabled(delete).tooltip(|_, cx| {
                cx.new(|_| Tooltip("Keep at least one profile".into()))
                    .into()
            })
        } else {
            editor_button(theme, delete, cx, |this, window, cx| {
                this.profile_editor.picker = None;
                this.profile_editor.confirm_delete = true;
                this.focus_overlay(window, cx);
                cx.notify();
            })
        };

        let changed = draft.changed(profile, cx);
        let discard = theme
            .button("discard-profile-changes", None)
            .child("Discard");
        let discard = if changed || self.profile_editor.picker.is_some() {
            editor_button(theme, discard, cx, move |this, _, cx| {
                this.profile_editor.picker = None;
                if let Some(mut draft) = this.profile_editor.drafts[index].take() {
                    draft.drop_imported_image(cx);
                }
                cx.notify();
            })
        } else {
            disabled(discard)
        };

        let save = theme
            .button("save-profile", Some(TEAL))
            .child("Save changes");
        let save = if changed && self.profile_editor.picker.is_none() {
            editor_button(theme, save, cx, move |this, _, cx| {
                this.save_profile_details(index, cx);
            })
        } else {
            disabled(save)
        };

        row()
            .flex_none()
            .flex_wrap()
            .gap(r(6.))
            .pt(r(8.))
            .border_t_1()
            .border_color(theme.color(STROKE))
            .child(delete)
            .child(row().flex_1())
            .child(discard)
            .child(save)
    }

    fn profile_delete_confirmation(
        &self,
        theme: Theme,
        index: usize,
        profile: &Profile,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let cancel = editor_button(
            theme,
            theme
                .button("cancel-profile-deletion", None)
                .child("Cancel"),
            cx,
            |this, window, cx| {
                this.profile_editor.cancel_delete();
                this.focus_overlay(window, cx);
                cx.notify();
            },
        );
        let delete = editor_button(
            theme,
            theme
                .button("confirm-profile-deletion", Some(RED))
                .child("Delete profile"),
            cx,
            move |this, window, cx| {
                if this.delete_profile(index, window, cx) {
                    if let Some(mut draft) = this.profile_editor.drafts.remove(index) {
                        draft.drop_imported_image(cx);
                    }
                    let selected = index.min(this.home.profiles().len() - 1);
                    this.profile_editor.selected = Some(selected);
                    this.profile_editor.cancel_delete();
                    this.focus_overlay(window, cx);
                    cx.notify();
                }
            },
        );
        col()
            .id("confirm-delete-profile")
            .flex_1()
            .overflow_y_scroll()
            .child(
                col()
                    .flex_none()
                    .gap(r(10.))
                    .p(r(14.))
                    .rounded(r(9.))
                    .border_1()
                    .border_color(theme.color(RED).opacity(0.30))
                    .bg(theme.color(RED).opacity(0.07))
                    .child(
                        theme
                            .title(format!("Delete {}?", profile.name(index)))
                            .text_size(r(18.)),
                    )
                    .child(
                        theme.label("This permanently removes the profile and its widget layout."),
                    )
                    .child(row().gap(r(6.)).flex_wrap().child(cancel).child(delete)),
            )
    }

    fn choose_profile_badge(&mut self, index: usize, badge: Option<Badge>, cx: &mut Context<Self>) {
        self.profile_editor.picker = None;
        let draft = self.profile_editor.drafts[index].as_mut().unwrap();
        draft.drop_imported_image(cx);
        draft.badge = badge;
        cx.notify();
    }

    fn choose_profile_image(&mut self, index: usize, cx: &mut Context<Self>) {
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use as profile image".into()),
        });
        self.profile_editor.picker = Some(cx.spawn(async move |this, cx| {
            let imported = match result.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => blocking::unblock(move || import_profile_image(&path))
                        .await
                        .map(Some),
                    None => Ok(None),
                },
                Ok(Err(error)) => Err(error),
                _ => Ok(None),
            };
            let _ = this.update(cx, |this, cx| {
                this.profile_editor.picker = None;
                match imported {
                    Ok(Some((badge, image))) => {
                        this.choose_profile_badge(index, Some(badge), cx);
                        this.profile_editor.drafts[index]
                            .as_mut()
                            .unwrap()
                            .imported_image = Some(image);
                    }
                    Ok(None) => {}
                    Err(error) => this.set_error(format!("{error:#}"), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save_profile_details(&mut self, index: usize, cx: &mut Context<Self>) {
        let draft = self.profile_editor.drafts[index].as_ref().unwrap();
        let profile = &self.home.profiles()[index];
        let details = ProfileDetails {
            name: draft.name(cx).map(str::to_owned),
            badge: draft.badge.clone(),
            image: draft.image(profile).cloned(),
        };
        if self.set_profile_details(index, details, cx) {
            self.profile_editor.drafts[index]
                .as_mut()
                .unwrap()
                .imported_image = None;
        }
    }
}

fn editor_button(
    theme: Theme,
    button: Stateful<Div>,
    cx: &Context<Dashboard>,
    action: impl Fn(&mut Dashboard, &mut Window, &mut Context<Dashboard>) + 'static,
) -> Stateful<Div> {
    let view = cx.entity();
    theme.bind(button, move |window, cx| {
        view.update(cx, |this, cx| action(this, window, cx))
    })
}
