use std::sync::Arc;

use gpui::{
    App, Bounds, Context, DispatchPhase, Div, Entity, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathPromptOptions, Pixels, Point, RenderImage,
    ScrollHandle, Stateful, Task, Window, canvas, div, prelude::*, px,
};

use super::{
    Dashboard, Tooltip,
    home::{
        Badge, MAX_PROFILES, Profile, ProfileDetails, ProfilePreset, import_profile_image,
        moved_profile_index,
    },
    icons::{Icon, ProfileIcon, icon},
    overlay::Overlay,
    text_input::{TextInput, field},
    theme::*,
    touch::{DragTarget, offer_drag},
};

#[derive(Default)]
pub(super) struct ProfileEditor {
    selected: Option<usize>,
    drafts: Vec<Option<Draft>>,
    picker: Option<Task<()>>,
    confirm_delete: bool,
    creation: Option<ProfileCreation>,
    list: ScrollHandle,
    drag: Option<ProfileDrag>,
}

#[derive(Clone, Copy)]
struct ProfileCreation {
    preset: usize,
    return_to_editor: bool,
}

struct ProfileDrag {
    from: usize,
    to: Option<usize>,
    chip: Bounds<Pixels>,
    start: Point<Pixels>,
    pointer: Point<Pixels>,
}

impl ProfileDrag {
    fn destination(&self, pointer: Point<Pixels>, list: &ScrollHandle, count: usize) -> usize {
        let center = self.chip.center().y + (pointer.y - self.start.y) - list.offset().y;
        (0..count)
            .filter_map(|slot| Some((slot, list.bounds_for_item(slot)?)))
            .min_by_key(|(_, bounds)| (bounds.center().y - center).abs())
            .map_or(self.from, |(slot, _)| slot)
    }
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
    pub(super) fn show_profile_presets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let return_to_editor = self.overlay.open == Some(Overlay::Profile);
        self.profile_editor.picker = None;
        self.profile_editor.creation = Some(ProfileCreation {
            preset: 0,
            return_to_editor,
        });
        self.show_overlay(Overlay::Profile, window, cx);
        if return_to_editor {
            self.focus_overlay(window, cx);
        }
    }

    pub(super) fn dismiss_profile_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.profile_editor.drag.take().is_some() {
            cx.notify();
            return true;
        }
        if self.profile_editor.creation.is_some() {
            self.finish_profile_creation(window, cx);
        } else if self.profile_editor.cancel_delete() {
            self.focus_overlay(window, cx);
            cx.notify();
        } else {
            return false;
        }
        true
    }

    fn finish_profile_creation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .profile_editor
            .creation
            .take()
            .is_some_and(|creation| creation.return_to_editor)
        {
            self.focus_overlay(window, cx);
            cx.notify();
        } else {
            self.close_overlay(window, cx);
        }
    }

    fn profile_presets_panel(
        &self,
        creation: ProfileCreation,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let choices = ProfilePreset::ALL
            .iter()
            .enumerate()
            .map(|(index, preset)| {
                let selected = index == creation.preset;
                let symbol = preset.icon.map_or_else(
                    || icon(Icon::Plus, theme.color(MUTED), 19.).into_any_element(),
                    |symbol| symbol.element(theme, 22.).into_any_element(),
                );
                let button = theme
                    .button(("profile-preset", index), selected.then_some(TEAL))
                    .h_full()
                    .justify_start()
                    .gap(r(10.))
                    .px(r(9.))
                    .child(
                        row()
                            .size(r(36.))
                            .flex_none()
                            .justify_center()
                            .rounded(r(7.))
                            .bg(theme
                                .color(if selected { TEAL } else { MUTED })
                                .opacity(0.10))
                            .child(symbol),
                    )
                    .child(
                        col()
                            .flex_1()
                            .child(
                                theme
                                    .label(preset.name)
                                    .text_color(theme.color(INK))
                                    .line_height(r(16.))
                                    .truncate(),
                            )
                            .child(
                                theme
                                    .quiet(preset.description)
                                    .line_height(r(15.))
                                    .line_clamp(2),
                            ),
                    )
                    .child(row().size(r(15.)).flex_none().when(selected, |it| {
                        it.child(icon(Icon::Check, theme.color(TEAL), 15.))
                    }));
                editor_button(theme, button, cx, move |this, _, cx| {
                    if let Some(creation) = &mut this.profile_editor.creation {
                        creation.preset = index;
                        cx.notify();
                    }
                })
            });
        let cancel = editor_button(
            theme,
            theme.button("cancel-new-profile", None).child("Cancel"),
            cx,
            |this, window, cx| this.finish_profile_creation(window, cx),
        );
        let create = editor_button(
            theme,
            theme
                .button("create-profile", Some(TEAL))
                .child("Create profile"),
            cx,
            |this, window, cx| {
                if let Some(creation) = this.profile_editor.creation
                    && let Some(index) =
                        this.add_profile(ProfilePreset::ALL[creation.preset], window, cx)
                {
                    if creation.return_to_editor {
                        this.profile_editor.select(index);
                    }
                    this.finish_profile_creation(window, cx);
                }
            },
        );

        self.overlay_panel(Overlay::Profile, theme.title("New profile"), cx)
            .child(
                col()
                    .id("profile-presets")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .grid()
                            .grid_cols(2)
                            .grid_rows(4)
                            .h_full()
                            .min_h(r(216.))
                            .max_h(r(256.))
                            .flex_none()
                            .gap(r(8.))
                            .children(choices),
                    ),
            )
            .child(
                row()
                    .flex_none()
                    .justify_end()
                    .gap(r(6.))
                    .pt(r(8.))
                    .border_t_1()
                    .border_color(theme.color(STROKE))
                    .child(cancel)
                    .child(create),
            )
    }

    pub(super) fn profile_editor_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(creation) = self.profile_editor.creation {
            return self.profile_presets_panel(creation, cx);
        }
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
            .child(self.track_profile_drag(cx))
    }

    fn profile_list(
        &self,
        theme: Theme,
        selected: usize,
        active: usize,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let profiles = self.home.profiles();
        let drag = self.profile_editor.drag.as_ref();
        let moving = drag.and_then(|drag| drag.to.map(|to| (drag.from, to)));
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
                this.show_profile_presets(window, cx)
            })
        };

        col()
            .w(r(188.))
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
                    .track_scroll(&self.profile_editor.list)
                    .gap(r(5.))
                    .children((0..profiles.len()).map(|slot| {
                        let index =
                            moving.map_or(slot, |(from, to)| moved_profile_index(slot, to, from));
                        if moving.is_some_and(|(from, _)| from == index) {
                            return div()
                                .h(r(38.))
                                .flex_none()
                                .rounded(r(8.))
                                .border_1()
                                .border_dashed()
                                .border_color(theme.color(TEAL).opacity(0.5))
                                .bg(theme.color(TEAL).opacity(0.07))
                                .into_any_element();
                        }
                        let profile = &profiles[index];
                        let name = profile.name(index);
                        let tooltip = if index == active {
                            format!("{name} (active)")
                        } else {
                            name
                        };
                        let grip = profile_grip(theme, index);
                        let grip = if profiles.len() > 1 {
                            grip.cursor_grab()
                                .hover(move |it| it.bg(theme.color(HOVER)))
                                .tooltip(|_, cx| {
                                    cx.new(|_| Tooltip("Drag to reorder".into())).into()
                                })
                                .on_mouse_move(|_, _, _| offer_drag(DragTarget::Free))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        this.start_profile_drag(index, event.position, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                        } else {
                            grip.opacity(0.4)
                        };
                        let label = profile_chip_label(theme, profile, index, index == active)
                            .id(format!("profile-label-{tooltip}"))
                            .tooltip(move |_, cx| {
                                cx.new(|_| Tooltip(tooltip.clone().into())).into()
                            });
                        editor_button(
                            theme,
                            profile_chip(theme, ("edit-profile-entry", index), index == selected)
                                .child(label)
                                .child(grip)
                                .on_key_down(cx.listener(move |this, event, window, cx| {
                                    this.reorder_profile_with_key(index, event, window, cx);
                                })),
                            cx,
                            move |this, _, cx| {
                                this.profile_editor.select(index);
                                cx.notify();
                            },
                        )
                        .into_any_element()
                    })),
            )
            .child(add)
    }

    pub(super) fn dragged_profile_chip(&self, theme: Theme) -> Option<Div> {
        let drag = self.profile_editor.drag.as_ref()?;
        drag.to?;
        let index = drag.from;
        let profile = self.home.profiles().get(index)?;
        let selected = self.profile_editor.selected == Some(index);
        let active = self.home.active_profile().0 == index;
        let origin = drag.chip.origin + (drag.pointer - drag.start);
        Some(
            div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .w(drag.chip.size.width)
                .h(drag.chip.size.height)
                .rounded(r(8.))
                .bg(theme.color(PANEL))
                .shadow_md()
                .cursor_grabbing()
                .occlude()
                .child(
                    profile_chip(theme, "dragged-profile-chip", selected)
                        .size_full()
                        .border_color(theme.color(TEAL))
                        .child(profile_chip_label(theme, profile, index, active))
                        .child(profile_grip(theme, index)),
                ),
        )
    }

    fn start_profile_drag(&mut self, index: usize, start: Point<Pixels>, cx: &mut Context<Self>) {
        let editor = &mut self.profile_editor;
        if let Some(mut chip) = editor.list.bounds_for_item(index) {
            chip.origin += editor.list.offset();
            editor.drag = Some(ProfileDrag {
                from: index,
                to: None,
                chip,
                start,
                pointer: start,
            });
            cx.notify();
        }
    }

    fn track_profile_drag(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let movement = view.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        movement.update(cx, |this, cx| this.move_profile_drag(event, cx));
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        view.update(cx, |this, cx| this.finish_profile_drag(event, window, cx));
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    fn move_profile_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let editor = &mut self.profile_editor;
        let Some(drag) = &mut editor.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            editor.drag = None;
        } else {
            let distance = event.position - drag.start;
            if drag.to.is_none() && distance.x.abs().max(distance.y.abs()) < px(3.) {
                return;
            }
            drag.pointer = event.position;
            drag.to =
                Some(drag.destination(event.position, &editor.list, self.home.profiles().len()));
        }
        cx.notify();
    }

    fn finish_profile_drag(
        &mut self,
        event: &MouseUpEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.profile_editor.drag.take() else {
            return;
        };
        let viewport = Bounds::new(Point::default(), window.viewport_size());
        if drag.to.is_some() && viewport.contains(&event.position) {
            let to = drag.destination(
                event.position,
                &self.profile_editor.list,
                self.home.profiles().len(),
            );
            self.reorder_profile(drag.from, to, cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn reorder_profile(&mut self, from: usize, to: usize, cx: &mut Context<Self>) -> bool {
        if !self.move_profile(from, to, cx) {
            return false;
        }
        let editor = &mut self.profile_editor;
        let draft = editor.drafts.remove(from);
        editor.drafts.insert(to, draft);
        editor.selected = editor
            .selected
            .map(|index| moved_profile_index(index, from, to));
        true
    }

    fn reorder_profile_with_key(
        &mut self,
        index: usize,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = match event.keystroke.key.as_str() {
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        if self.profile_editor.drag.is_some() || event.keystroke.modifiers.modified() {
            return;
        }
        cx.stop_propagation();
        if let Some(to) = index.checked_add_signed(step)
            && self.reorder_profile(index, to, cx)
        {
            if step < 0 {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
        }
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
            editor_button(theme, import, cx, |this, _, cx| {
                this.choose_profile_image(cx);
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

    fn choose_profile_image(&mut self, cx: &mut Context<Self>) {
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
                    Ok(Some((badge, image))) if let Some(index) = this.profile_editor.selected => {
                        this.choose_profile_badge(index, Some(badge), cx);
                        this.profile_editor.drafts[index]
                            .as_mut()
                            .unwrap()
                            .imported_image = Some(image);
                    }
                    Ok(_) => {}
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

fn profile_chip(theme: Theme, id: impl Into<gpui::ElementId>, selected: bool) -> Stateful<Div> {
    theme
        .button(id, selected.then_some(TEAL))
        .h(r(38.))
        .pl(r(7.))
        .pr(r(3.))
        .gap(r(6.))
        .justify_start()
}

fn profile_chip_label(theme: Theme, profile: &Profile, index: usize, active: bool) -> Div {
    row()
        .h_full()
        .flex_1()
        .gap(r(6.))
        .child(
            row()
                .size(r(24.))
                .flex_none()
                .justify_center()
                .child(profile.button_content(index, theme, 18.)),
        )
        .child(col().flex_1().truncate().child(profile.name(index)))
        .when(active, |label| label.child(theme.dot(TEAL)))
}

fn profile_grip(theme: Theme, index: usize) -> Stateful<Div> {
    row()
        .id(("profile-drag-handle", index))
        .w(r(30.))
        .h_full()
        .flex_none()
        .justify_center()
        .rounded(r(6.))
        .child(icon(Icon::Grip, theme.color(MUTED), 18.))
}

fn editor_button(
    theme: Theme,
    button: Stateful<Div>,
    cx: &Context<Dashboard>,
    action: impl Fn(&mut Dashboard, &mut Window, &mut Context<Dashboard>) + 'static,
) -> Stateful<Div> {
    let view = cx.entity();
    theme.bind(button, move |window, cx| {
        view.update(cx, |this, cx| {
            if this.profile_editor.drag.is_none() {
                action(this, window, cx);
            }
        });
    })
}
