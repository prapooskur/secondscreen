use gpui::{Context, IntoElement, SharedString, Window, prelude::*};

use crate::config::HomeAssistantCredentials;

use super::{Binding, Connection, ConnectionFields, Picker};
use crate::ui::{
    Dashboard, Section,
    text_input::{TextInput, field},
    theme::*,
};

impl Dashboard {
    pub(in crate::ui) fn choose_home_assistant(
        &self,
        widget: u32,
        label: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity();
        theme.bind(
            theme
                .button(("choose-ha-entity", widget as usize), None)
                .h(r(26.))
                .px(r(7.))
                .child(label),
            move |window, cx| {
                view.update(cx, |this, cx| {
                    this.open_home_assistant_picker(widget, window, cx)
                })
            },
        )
    }

    fn open_home_assistant_picker(
        &mut self,
        widget: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(Section::HomeAssistant, window, cx);
        let search = cx.new(|cx| TextInput::with_placeholder("Search names or entity IDs", cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        self.home_assistant.picker = Some(Picker {
            widget,
            profile: self.home.active_profile().0,
            search,
        });
    }

    pub(in crate::ui) fn home_assistant_settings(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if let (Some(picker), Some(credentials)) = (
            self.home_assistant.picker.as_ref(),
            self.preferences.home_assistant.as_ref(),
        ) {
            self.home_assistant_picker(picker, &credentials.url, cx)
                .into_any_element()
        } else {
            self.home_assistant_connection(cx).into_any_element()
        }
    }

    fn home_assistant_connection(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let saved = self.preferences.home_assistant.as_ref();
        let fields = self
            .home_assistant
            .fields
            .get_or_insert_with(|| ConnectionFields {
                url: cx.new(|cx| {
                    let mut input = TextInput::with_placeholder("http://192.168.1.10:8123", cx);
                    if let Some(saved) = saved {
                        input.set_value(saved.url.clone(), cx);
                    }
                    input
                }),
                token: cx.new(|cx| {
                    TextInput::new(
                        saved.map_or_else(String::new, |saved| saved.token.clone()),
                        true,
                        cx,
                    )
                }),
            });
        let (url, token) = (fields.url.clone(), fields.token.clone());
        let testing = self.home_assistant.verification.is_some();
        let view = cx.entity();
        let connect = theme.button("ha-connect", Some(BLUE)).child(if testing {
            "Connecting…"
        } else {
            "Connect"
        });
        let connect = if testing {
            disabled(connect)
        } else {
            let (url, token) = (url.clone(), token.clone());
            let view = view.clone();
            theme.bind(connect, move |_, cx| {
                let result = HomeAssistantCredentials::from_input(
                    &url.read(cx).value,
                    &token.read(cx).value,
                );
                view.update(cx, |this, cx| this.verify_home_assistant(result, cx));
            })
        };
        col()
            .flex_none()
            .gap(r(9.))
            .child(theme.title("Home Assistant"))
            .child(theme.quiet(
                "Create a long-lived access token in your Home Assistant profile under Security.",
            ))
            .child(
                split()
                    .gap(r(9.))
                    .child(field(theme, "Server URL", url))
                    .child(field(theme, "Access token", token)),
            )
            .child(
                row()
                    .gap(r(7.))
                    .child(connect)
                    .when(saved.is_some(), |row| {
                        let forget = theme.button("ha-forget", None).child("Disconnect");
                        row.child(if testing {
                            disabled(forget)
                        } else {
                            theme.bind(forget, move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.disconnect_home_assistant(window, cx)
                                });
                            })
                        })
                    }),
            )
            .children(
                self.home_assistant
                    .verification_error
                    .as_ref()
                    .map(|error| theme.quiet(error.clone()).text_color(theme.color(RED))),
            )
            .when(saved.is_some(), |body| {
                body.child(self.home_assistant_status(theme))
            })
    }

    fn home_assistant_status(&self, theme: Theme) -> impl IntoElement {
        let integration = &self.home_assistant;
        if let Some(error) = &integration.connection_error {
            theme.quiet(error.clone()).text_color(theme.color(RED))
        } else if let Some(devices) = &integration.devices {
            let count = devices.len();
            let noun = if count == 1 { "entity" } else { "entities" };
            theme.quiet(format!("Connected · {count} controllable {noun}"))
        } else {
            theme.quiet("Connecting to Home Assistant…")
        }
    }

    fn verify_home_assistant(
        &mut self,
        credentials: anyhow::Result<HomeAssistantCredentials>,
        cx: &mut Context<Self>,
    ) {
        if self.home_assistant.verification.is_some() {
            return;
        }
        self.home_assistant.verification_error = None;
        cx.notify();
        let credentials = match credentials {
            Ok(credentials) => credentials,
            Err(error) => {
                self.home_assistant.verification_error = Some(error.to_string());
                return;
            }
        };
        let testing = credentials.clone();
        self.home_assistant.verification = Some(cx.spawn(async move |this, cx| {
            let result = blocking::unblock(move || {
                let connection = Connection::new(&testing)?;
                let devices = connection.entities()?;
                Ok::<_, anyhow::Error>((connection, devices))
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                this.home_assistant.verification = None;
                match result {
                    Ok(verified) => {
                        let mut preferences = this.preferences.clone();
                        preferences.home_assistant = Some(credentials.clone());
                        if this.save_preferences(preferences, cx) {
                            this.start_home_assistant(credentials, Some(verified), cx);
                        }
                    }
                    Err(error) => {
                        this.home_assistant.verification_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        }));
    }

    fn disconnect_home_assistant(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.home_assistant.verification.is_some() {
            return;
        }
        let mut preferences = self.preferences.clone();
        preferences.home_assistant = None;
        if self.save_preferences(preferences, cx) {
            self.focus_overlay(window, cx);
            self.home_assistant.fields = None;
            self.home_assistant.verification_error = None;
            self.home_assistant.stop_worker();
        }
    }

    fn home_assistant_picker(
        &self,
        picker: &Picker,
        instance: &str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::current(cx);
        let view = cx.entity();
        let search = picker.search.read(cx).value.trim().to_lowercase();
        let devices = self.home_assistant.devices.as_deref().unwrap_or_default();
        let matches = devices.iter().filter(|device| {
            device.id.contains(&search) || device.name().to_lowercase().contains(&search)
        });
        let current = self
            .home
            .home_assistant_binding(picker.widget)
            .filter(|binding| binding.instance == instance);
        let (profile, widget) = (picker.profile, picker.widget);
        col()
            .flex_none()
            .gap(r(6.))
            .child(
                row()
                    .justify_between()
                    .child(theme.title("Choose an entity"))
                    .child(
                        theme.bind(
                            theme
                                .button("ha-connection-settings", None)
                                .h(r(28.))
                                .child("Connection"),
                            move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.open_settings(Section::HomeAssistant, window, cx)
                                });
                            },
                        ),
                    ),
            )
            .child(self.home_assistant_status(theme))
            .child(picker.search.clone())
            .when(
                !search.is_empty() && !devices.is_empty() && matches.clone().next().is_none(),
                |body| body.child(theme.quiet("No entities match your search.")),
            )
            .children(matches.map(|device| {
                let id = device.id.clone();
                let active = current.is_some_and(|binding| binding.entity == id);
                let instance = instance.to_owned();
                let view = cx.entity();
                theme.bind(
                    theme
                        .button(SharedString::from(id.clone()), active.then_some(BLUE))
                        .min_h(r(45.))
                        .justify_start()
                        .child(
                            col()
                                .flex_1()
                                .child(theme.label(device.name().to_owned()).truncate())
                                .child(theme.quiet(id.clone()).truncate()),
                        )
                        .when(active, |button| button.child("Selected")),
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            let binding = Binding {
                                instance: instance.clone(),
                                entity: id.clone(),
                            };
                            if this.set_home_assistant_binding(profile, widget, binding, cx) {
                                this.close_overlay(window, cx);
                            }
                        });
                    },
                )
            }))
    }
}
