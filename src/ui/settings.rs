use std::{collections::BTreeMap, time::Instant};

use gpui::{Context, Entity, IntoElement, SharedString, Styled, Window, div, prelude::*};

use crate::{
    config::{DiscordCredentials, Preferences},
    model::{AudioChannel, AudioKind, DiscordAction},
};

use super::{
    Dashboard, Page, Tooltip,
    credential_input::CredentialInput,
    icons::{Icon, icon},
    monitor::capacity,
    theme::*,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    General,
    Media,
    Audio,
    Storage,
    Discord,
}

impl Section {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Media,
        Self::Audio,
        Self::Storage,
        Self::Discord,
    ];

    fn presentation(self) -> (&'static str, Icon, u32) {
        match self {
            Self::General => ("General", Icon::Settings, TEAL),
            Self::Media => ("Media apps", Icon::Music, 0xf2799c),
            Self::Audio => ("Audio devices", Icon::Speaker, AMBER),
            Self::Storage => ("Storage", Icon::Disk, BLUE),
            Self::Discord => ("Discord", Icon::Discord, VIOLET),
        }
    }

    fn hidden(self, preferences: &mut Preferences) -> &mut BTreeMap<String, String> {
        match self {
            Self::Media => &mut preferences.hidden_media,
            Self::Audio => &mut preferences.hidden_audio,
            Self::Storage => &mut preferences.hidden_disks,
            _ => unreachable!(),
        }
    }
}

#[derive(Default)]
pub(super) struct SettingsView {
    section: Section,
    discord_fields: Option<DiscordFields>,
    discord_saved: bool,
}

struct DiscordFields {
    id: Entity<CredentialInput>,
    secret: Entity<CredentialInput>,
    token: Entity<CredentialInput>,
}

struct Choice {
    name: String,
    detail: String,
    hidden: bool,
}

pub(super) fn audio_device_key(channel: &AudioChannel) -> Option<String> {
    let direction = match channel.id.kind {
        AudioKind::Output => "output",
        AudioKind::Input => "input",
        _ => return None,
    };
    Some(format!("{direction}:{}", channel.id.name))
}

fn audio_device_type(key: &str) -> &'static str {
    if key.starts_with("input:") {
        "Microphone"
    } else {
        "Output"
    }
}

impl Dashboard {
    pub(super) fn open_settings(
        &mut self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.section = section;
        self.select(Page::Settings, window, cx);
        cx.notify();
    }

    pub(super) fn settings_page(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.settings.section;
        let view = cx.entity();
        let sidebar = panel()
            .id("settings-categories")
            .w(r(190.))
            .flex_none()
            .gap(r(5.))
            .p(r(9.))
            .children(Section::ALL.map(|section| {
                let (name, symbol, accent) = section.presentation();
                let active = section == self.settings.section;
                let view = view.clone();
                bind(
                    button(name, active.then_some(accent))
                        .justify_start()
                        .h(r(39.))
                        .child(icon(symbol, c(if active { accent } else { MUTED }), 16.))
                        .child(name),
                    move |window, cx| {
                        view.update(cx, |this, cx| this.open_settings(section, window, cx))
                    },
                )
            }));
        let body = match section {
            Section::General => self.general_settings(cx).into_any_element(),
            Section::Discord => self.discord_settings(cx).into_any_element(),
            _ => self.visibility_settings(section, cx).into_any_element(),
        };
        col()
            .gap(r(12.))
            .flex_1()
            .child(section_header("Settings", None))
            .child(
                split().flex_1().min_h_0().gap(r(12.)).child(sidebar).child(
                    panel().flex_1().overflow_hidden().child(
                        col()
                            .id(("settings-content", section as usize))
                            .flex_1()
                            .overflow_y_scroll()
                            .p(r(14.))
                            .child(body),
                    ),
                ),
            )
    }

    fn general_settings(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let layout = view.clone();
        col()
            .gap(r(12.))
            .flex_none()
            .child(title("General"))
            .child(preference_row(
                "start-fullscreen-row",
                "Start fullscreen",
                "Applies the next time Second Screen opens.",
                bind(
                    choice_button(
                        "startup-fullscreen",
                        self.preferences.start_fullscreen,
                        "On",
                        "Off",
                    ),
                    move |_, cx| {
                        view.update(cx, |this, cx| {
                            let mut preferences = this.preferences.clone();
                            preferences.start_fullscreen = !preferences.start_fullscreen;
                            this.save_preferences(preferences, cx);
                        });
                    },
                ),
            ))
            .child(preference_row(
                "home-layout-row",
                "Home layout",
                "Add, arrange, and resize widgets.",
                bind(
                    button("settings-edit-home", None).child("Edit layout"),
                    move |window, cx| {
                        layout.update(cx, |this, cx| {
                            this.select(Page::Home, window, cx);
                            this.home.edit();
                        });
                    },
                ),
            ))
    }

    fn visibility_settings(&self, section: Section, cx: &Context<Self>) -> impl IntoElement {
        let (title_text, hint, empty) = match section {
            Section::Media => (
                "Media apps",
                "Choose which apps appear in Media and Now Playing.",
                "Open a media app that supports MPRIS to see it here.",
            ),
            Section::Audio => (
                "Audio devices",
                "Choose which outputs and microphones appear in Audio and on Home.",
                "Connect an audio device to see it here.",
            ),
            Section::Storage => (
                "Storage",
                "Choose which mounted volumes appear in System and on Home.",
                "Mounted volumes with capacity information appear here.",
            ),
            _ => unreachable!(),
        };
        let (hidden, detected): (&BTreeMap<_, _>, Vec<_>) = match section {
            Section::Media => (
                &self.preferences.hidden_media,
                self.media
                    .players
                    .iter()
                    .map(|player| {
                        (
                            player.app_id.clone(),
                            (player.name.clone(), player.app_id.clone()),
                        )
                    })
                    .collect(),
            ),
            Section::Audio => (
                &self.preferences.hidden_audio,
                self.audio
                    .channels
                    .iter()
                    .filter_map(|channel| {
                        let key = audio_device_key(channel)?;
                        let detail = audio_device_type(&key).to_owned();
                        Some((key, (channel.label.clone(), detail)))
                    })
                    .collect(),
            ),
            Section::Storage => (
                &self.preferences.hidden_disks,
                self.metrics
                    .storage
                    .iter()
                    .map(|disk| {
                        (
                            disk.mount.clone(),
                            (
                                disk.mount.clone(),
                                format!("{} total", capacity(disk.total)),
                            ),
                        )
                    })
                    .collect(),
            ),
            _ => unreachable!(),
        };
        let mut choices = BTreeMap::new();
        for (key, name) in hidden {
            choices.insert(
                key.clone(),
                Choice {
                    name: name.clone(),
                    detail: "Unavailable".into(),
                    hidden: true,
                },
            );
        }
        for (key, (name, detail)) in detected {
            choices.insert(
                key.clone(),
                Choice {
                    name,
                    detail,
                    hidden: hidden.contains_key(&key),
                },
            );
        }
        let mut choices: Vec<_> = choices.into_iter().collect();
        choices.sort_by(|a, b| {
            a.1.name
                .to_lowercase()
                .cmp(&b.1.name.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        });

        col()
            .gap(r(9.))
            .flex_none()
            .child(title(title_text))
            .child(quiet(hint))
            .when(choices.is_empty(), |it| it.child(quiet(empty).py(r(10.))))
            .children(choices.into_iter().map(|(key, choice)| {
                let view = cx.entity();
                let name = choice.name;
                let saved_name = name.clone();
                let id = SharedString::from(key.clone());
                preference_row(
                    id.clone(),
                    name,
                    choice.detail,
                    bind(
                        choice_button(id, !choice.hidden, "Shown", "Hidden"),
                        move |_, cx| {
                            view.update(cx, |this, cx| {
                                let mut preferences = this.preferences.clone();
                                let hidden = section.hidden(&mut preferences);
                                if hidden.remove(&key).is_none() {
                                    hidden.insert(key.clone(), saved_name.clone());
                                }
                                this.save_preferences(preferences, cx);
                            });
                        },
                    ),
                )
            }))
    }

    fn discord_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = self.settings.discord_fields.get_or_insert_with(|| {
            let saved = self
                .preferences
                .discord
                .clone()
                .filter(DiscordCredentials::valid)
                .unwrap_or_default();
            DiscordFields {
                id: cx.new(|cx| CredentialInput::new(saved.client_id, false, cx)),
                secret: cx
                    .new(|cx| CredentialInput::new(saved.secret.unwrap_or_default(), true, cx)),
                token: cx.new(|cx| {
                    CredentialInput::new(saved.access_token.unwrap_or_default(), true, cx)
                }),
            }
        });
        let (id, secret, token) = (
            fields.id.clone(),
            fields.secret.clone(),
            fields.token.clone(),
        );
        let view = cx.entity();
        let clear = view.clone();
        let from_env = view.clone();
        let open = view.clone();
        let save = bind(
            button("save-discord-settings", Some(VIOLET)).child("Save credentials"),
            {
                let (id, secret, token) = (id.clone(), secret.clone(), token.clone());
                move |window, cx| {
                    let result = DiscordCredentials::from_input(
                        &id.read(cx).value,
                        &secret.read(cx).value,
                        &token.read(cx).value,
                    );
                    view.update(cx, |this, cx| match result {
                        Ok(credentials) => this.save_discord(Some(credentials), window, cx),
                        Err(error) => {
                            this.error = Some((Instant::now(), error.to_string()));
                            cx.notify();
                        }
                    });
                }
            },
        );
        let saved = self
            .preferences
            .discord
            .as_ref()
            .is_some_and(DiscordCredentials::valid);
        let environment = DiscordCredentials::from_env().is_some();
        let using_environment = environment && self.preferences.discord.is_none();
        let note = if using_environment {
            "Using credentials from the environment."
        } else if self.settings.discord_saved {
            "Credentials saved. Connect from Discord voice."
        } else {
            "Enter your application ID and either a client secret or an access token."
        };
        col()
            .gap(r(10.))
            .flex_none()
            .child(title("Discord voice RPC"))
            .child(quiet(note))
            .child(field("Application ID", id))
            .child(
                split()
                    .gap(r(10.))
                    .child(field("Client secret", secret))
                    .child(field("Access token", token)),
            )
            .child(
                row()
                    .gap(r(7.))
                    .flex_wrap()
                    .child(save)
                    .when(saved || using_environment, |row| {
                        row.child(bind(
                            button("clear-discord-settings", None).child("Clear credentials"),
                            move |window, cx| {
                                clear.update(cx, |this, cx| {
                                    this.save_discord(
                                        Some(DiscordCredentials::default()),
                                        window,
                                        cx,
                                    );
                                });
                            },
                        ))
                    })
                    .when(environment && !using_environment, |row| {
                        row.child(bind(
                            button("use-discord-environment", None).child("Use environment"),
                            move |window, cx| {
                                from_env.update(cx, |this, cx| this.save_discord(None, window, cx))
                            },
                        ))
                    })
                    .when(saved || using_environment, |row| {
                        row.child(bind(
                            button("open-discord-controls", None).child("Discord voice"),
                            move |window, cx| {
                                open.update(cx, |this, cx| this.select(Page::Discord, window, cx))
                            },
                        ))
                    }),
            )
    }

    fn save_discord(
        &mut self,
        credentials: Option<DiscordCredentials>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.preferences.discord != credentials;
        let mut preferences = self.preferences.clone();
        preferences.discord = credentials.clone();
        if self.save_preferences(preferences, cx) {
            self.focus_dashboard(window, cx);
            self.settings.discord_fields = None;
            self.settings.discord_saved =
                credentials.as_ref().is_some_and(DiscordCredentials::valid);
            if changed {
                self.send_discord(DiscordAction::Configure(credentials));
            }
        }
    }

    fn save_preferences(&mut self, preferences: Preferences, cx: &mut Context<Self>) -> bool {
        let saved = match preferences.save() {
            Ok(()) => {
                self.preferences = preferences;
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some((Instant::now(), format!("{error:#}")));
                false
            }
        };
        cx.notify();
        saved
    }
}

fn preference_row(
    id: impl Into<gpui::ElementId>,
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    control: impl IntoElement,
) -> impl IntoElement {
    let name = name.into();
    row()
        .id(id)
        .gap(r(10.))
        .flex_none()
        .p(r(8.))
        .rounded(r(8.))
        .border_1()
        .border_color(c(STROKE))
        .bg(c(BACK).opacity(0.44))
        .child(
            col()
                .flex_1()
                .gap(r(3.))
                .child(
                    label(name.clone())
                        .id("preference-label")
                        .text_color(c(INK))
                        .truncate()
                        .tooltip(move |_, cx| cx.new(|_| Tooltip(name.clone())).into()),
                )
                .child(quiet(detail.into()).truncate()),
        )
        .child(control)
}

fn choice_button(
    id: impl Into<gpui::ElementId>,
    enabled: bool,
    on: &str,
    off: &str,
) -> gpui::Stateful<gpui::Div> {
    button(id, enabled.then_some(TEAL))
        .w(r(84.))
        .h(r(30.))
        .px(r(6.))
        .child(div().child(if enabled {
            on.to_owned()
        } else {
            off.to_owned()
        }))
}

fn field(name: &'static str, input: Entity<CredentialInput>) -> impl IntoElement {
    col()
        .id(name)
        .flex_1()
        .gap(r(4.))
        .child(label(name))
        .child(input)
}
