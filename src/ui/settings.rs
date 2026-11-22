use std::{collections::BTreeMap, time::Instant};

use gpui::{
    Context, Entity, FocusHandle, IntoElement, SharedString, Styled, Window, div, prelude::*,
};

use crate::{
    config::{DISCORD_REDIRECT_URI, DiscordCredentials, Preferences},
    model::{AudioChannel, AudioKind, DiscordAction},
};

use super::{
    CONTENT_X, CONTENT_Y, Dashboard, Tooltip,
    icons::{Icon, icon},
    monitor::capacity,
    text_input::TextInput,
    theme::*,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    General,
    Widgets,
    Media,
    Audio,
    Storage,
    Discord,
}

impl Section {
    const ALL: [Self; 6] = [
        Self::General,
        Self::Widgets,
        Self::Media,
        Self::Audio,
        Self::Storage,
        Self::Discord,
    ];

    fn presentation(self) -> (&'static str, Icon, u32) {
        match self {
            Self::General => ("General", Icon::Settings, TEAL),
            Self::Widgets => ("Widgets", Icon::Home, AMBER),
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
            Self::Storage => &mut preferences.hidden_drives,
            _ => unreachable!(),
        }
    }
}

pub(super) struct SettingsView {
    pub(super) open: bool,
    pub(super) section: Section,
    pub(super) focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    discord_fields: Option<DiscordFields>,
    discord_saved: bool,
    widget_fields: Option<(Entity<TextInput>, Entity<TextInput>)>,
}

impl SettingsView {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            section: Section::default(),
            focus,
            previous_focus: None,
            discord_fields: None,
            discord_saved: false,
            widget_fields: None,
        }
    }
}

struct DiscordFields {
    id: Entity<TextInput>,
    secret: Entity<TextInput>,
    token: Entity<TextInput>,
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
        if !self.settings.open {
            self.settings.previous_focus = window.focused(cx);
            self.settings.open = true;
            self.gesture.borrow_mut().take();
            self.focus_settings(window, cx);
        }
        cx.notify();
    }

    fn focus_settings(&self, window: &mut Window, _cx: &mut Context<Self>) {
        #[cfg(feature = "ui-gpui")]
        window.focus(&self.settings.focus);
        #[cfg(not(feature = "ui-gpui"))]
        window.focus(&self.settings.focus, _cx);
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.open {
            self.settings.open = false;
            self.settings.widget_fields = None;
            let previous = self
                .settings
                .previous_focus
                .take()
                .filter(|focus| self.focus.contains(focus, window))
                .unwrap_or_else(|| self.focus.clone());
            #[cfg(feature = "ui-gpui")]
            window.focus(&previous);
            #[cfg(not(feature = "ui-gpui"))]
            window.focus(&previous, cx);
            cx.notify();
        }
    }

    pub(super) fn settings_overlay(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        split()
            .absolute()
            .inset_0()
            .justify_end()
            .px(r(CONTENT_X))
            .py(r(CONTENT_Y))
            .child(
                div()
                    .id("settings-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(c(BACK).opacity(0.52))
                    .occlude()
                    .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
            )
            .child(self.settings_panel(cx))
    }

    fn settings_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.settings.section;
        let view = cx.entity();
        let tabs = row()
            .flex_wrap()
            .id("settings-categories")
            .flex_none()
            .gap(r(5.))
            .children(Section::ALL.map(|section| {
                let (name, symbol, accent) = section.presentation();
                let active = section == self.settings.section;
                let view = view.clone();
                bind(
                    button(name, active.then_some(accent))
                        .justify_start()
                        .h(r(29.))
                        .px(r(8.))
                        .child(icon(symbol, c(if active { accent } else { MUTED }), 16.))
                        .child(name),
                    move |window, cx| {
                        view.update(cx, |this, cx| this.open_settings(section, window, cx))
                    },
                )
            }));
        let body = match section {
            Section::General => self.general_settings(cx).into_any_element(),
            Section::Widgets => self.widget_settings(cx).into_any_element(),
            Section::Discord => self.discord_settings(cx).into_any_element(),
            _ => self.visibility_settings(section, cx).into_any_element(),
        };
        panel()
            .id("settings-panel")
            .w(r(540.))
            .max_w_full()
            .h_full()
            .flex_none()
            .occlude()
            .track_focus(&self.settings.focus)
            .tab_index(0)
            .tab_stop(false)
            .tab_group()
            .overflow_hidden()
            .p(r(12.))
            .gap(r(9.))
            .child(
                row().justify_between().child(title("Settings")).child(bind(
                    button("dismiss-settings", None)
                        .size(r(28.))
                        .px_0()
                        .child(icon(Icon::Close, c(TEXT), 14.))
                        .tooltip(|_, cx| cx.new(|_| Tooltip("Close settings".into())).into()),
                    move |window, cx| view.update(cx, |this, cx| this.close_settings(window, cx)),
                )),
            )
            .child(tabs)
            .child(
                col()
                    .id(("settings-content", section as usize))
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
    }

    fn widget_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        use super::utilities::FPS_LAUNCH_OPTIONS;
        let (zones, location) = self
            .settings
            .widget_fields
            .get_or_insert_with(|| {
                let input = |placeholder, value, cx: &mut Context<Self>| {
                    cx.new(|cx| {
                        let mut input = TextInput::with_placeholder(placeholder, cx);
                        input.set_value(value, cx);
                        input
                    })
                };
                (
                    input(
                        "UTC, Europe/London",
                        super::utilities::clock_zones(&self.preferences.clock_zones).join(", "),
                        cx,
                    ),
                    input(
                        "City name or latitude, longitude",
                        self.preferences.weather_location.clone(),
                        cx,
                    ),
                )
            })
            .clone();
        let view = cx.entity();
        let save = bind(
            button("save-widget-settings", Some(TEAL)).child("Save locations"),
            {
                let (zones, location) = (zones.clone(), location.clone());
                move |_, cx| {
                    view.update(cx, |this, cx| {
                        let location = location.read(cx).value.trim().to_owned();
                        let result = super::utilities::parse_zones(&zones.read(cx).value).and_then(
                            |zones| {
                                super::utilities::coordinates(&location)?;
                                Ok(zones)
                            },
                        );
                        match result {
                            Ok(zones) => {
                                let mut prefs = this.preferences.clone();
                                prefs.clock_zones = Some(zones);
                                prefs.weather_location = location;
                                if this.save_preferences(prefs, cx) {
                                    this.settings.widget_fields = None;
                                }
                            }
                            Err(err) => {
                                this.error = Some((Instant::now(), err.to_string()));
                                cx.notify();
                            }
                        }
                    })
                }
            },
        );
        let weather_option = |id, text, toggle: fn(&mut Preferences)| {
            let view = cx.entity();
            bind(button(id, None).child(text), move |_, cx| {
                view.update(cx, |this, cx| {
                    let mut preferences = this.preferences.clone();
                    toggle(&mut preferences);
                    this.save_preferences(preferences, cx);
                })
            })
        };
        col()
            .gap(r(9.))
            .flex_none()
            .child(field("World clock time zones", zones))
            .child(quiet(
                "Separate IANA time zones with commas. Local time is always shown.",
            ))
            .child(field("Weather and air quality location", location))
            .child(row().child(save))
            .child(
                row()
                    .flex_wrap()
                    .gap(r(7.))
                    .child(weather_option(
                        "weather-units",
                        if self.preferences.fahrenheit {
                            "Temperature · °F"
                        } else {
                            "Temperature · °C"
                        },
                        |prefs| prefs.fahrenheit = !prefs.fahrenheit,
                    ))
                    .child(weather_option(
                        "weather-wind",
                        if self.preferences.weather_mph {
                            "Wind · mph"
                        } else {
                            "Wind · km/h"
                        },
                        |prefs| prefs.weather_mph = !prefs.weather_mph,
                    ))
                    .child(weather_option(
                        "weather-time",
                        if self.preferences.weather_12_hour {
                            "Sun times · 12-hour"
                        } else {
                            "Sun times · 24-hour"
                        },
                        |prefs| prefs.weather_12_hour = !prefs.weather_12_hour,
                    )),
            )
            .child(
                row()
                    .justify_between()
                    .child(label("Steam launch options for MangoHud FPS"))
                    .child(bind(
                        button("copy-launch-options", None)
                            .h(r(25.))
                            .px(r(7.))
                            .child("Copy"),
                        |_, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                FPS_LAUNCH_OPTIONS.to_owned(),
                            ))
                        },
                    )),
            )
            .child(quiet(
                "Install MangoHud to track current FPS, 1% low, and 0.1% low.",
            ))
            .child(quiet(FPS_LAUNCH_OPTIONS))
    }

    fn general_settings(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
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
    }

    fn visibility_settings(&self, section: Section, cx: &Context<Self>) -> impl IntoElement {
        let (title_text, hint, empty) = match section {
            Section::Media => (
                "Media apps",
                "Choose which apps appear in the Now playing widget.",
                "Open a media app that supports MPRIS to see it here.",
            ),
            Section::Audio => (
                "Audio devices",
                "Choose which outputs and microphones appear in the Audio widget.",
                "Connect an audio device to see it here.",
            ),
            Section::Storage => (
                "Storage",
                "Choose which drives appear in the Storage widget.",
                "Detected drives appear here.",
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
                &self.preferences.hidden_drives,
                self.metrics
                    .storage
                    .iter()
                    .map(|disk| {
                        let total = format!("{} total", capacity(disk.total));
                        let detail = if disk.device == disk.label {
                            total
                        } else {
                            format!("{} · {total}", disk.device)
                        };
                        (disk.id.clone(), (disk.label.clone(), detail))
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
                id: cx.new(|cx| TextInput::new(saved.client_id, false, cx)),
                secret: cx.new(|cx| TextInput::new(saved.secret.unwrap_or_default(), true, cx)),
                token: cx
                    .new(|cx| TextInput::new(saved.access_token.unwrap_or_default(), true, cx)),
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
            "Credentials saved. Connect from the Discord widget."
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
            .child(quiet(format!(
                "For client-secret login, add {DISCORD_REDIRECT_URI} under OAuth2 → Redirects in the Discord Developer Portal."
            )))
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
            self.focus_settings(window, cx);
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
                .child(quiet(detail.into())),
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

fn field(name: &'static str, input: Entity<TextInput>) -> impl IntoElement {
    col()
        .id(name)
        .flex_1()
        .gap(r(4.))
        .child(label(name))
        .child(input)
}
