use std::{
    collections::BTreeMap,
    io::{ErrorKind, Write},
    process::{Command, Stdio},
};

use gpui::{Context, Entity, IntoElement, SharedString, Styled, Window, div, prelude::*};

use crate::{
    config::{DISCORD_REDIRECT_URI, DiscordCredentials, Preferences, ThemePreference},
    model::{AudioChannel, AudioKind, DiscordAction},
    platform::{autostart, in_flatpak, launch_display},
};

use super::{
    Dashboard, Tooltip,
    icons::{Icon, icon},
    monitor::capacity,
    overlay::Overlay,
    text_input::{TextInput, field},
    theme::*,
    utilities::FPS_LAUNCH_OPTIONS,
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
    Notifications,
    Integrations,
    HomeAssistant,
}

impl Section {
    const ALL: [Self; 9] = [
        Self::General,
        Self::Widgets,
        Self::Notifications,
        Self::Discord,
        Self::HomeAssistant,
        Self::Integrations,
        Self::Media,
        Self::Audio,
        Self::Storage,
    ];

    fn presentation(self) -> (&'static str, Icon, Color) {
        match self {
            Self::General => ("General", Icon::Settings, TEAL),
            Self::Widgets => ("Widgets", Icon::Home, AMBER),
            Self::Media => ("Media apps", Icon::Music, PINK),
            Self::Audio => ("Audio devices", Icon::Speaker, AMBER),
            Self::Storage => ("Storage", Icon::Disk, BLUE),
            Self::Discord => ("Discord", Icon::Discord, VIOLET),
            Self::Notifications => ("Notifications", Icon::Bell, BLUE),
            Self::Integrations => ("Integration status", Icon::Activity, TEAL),
            Self::HomeAssistant => ("Home Assistant", Icon::Home, BLUE),
        }
    }

    fn hidden(self, preferences: &mut Preferences) -> &mut BTreeMap<String, String> {
        match self {
            Self::Media => &mut preferences.hidden_media,
            Self::Audio => &mut preferences.hidden_audio,
            Self::Storage => &mut preferences.hidden_drives,
            Self::Notifications => &mut preferences.hidden_notification_apps,
            _ => unreachable!(),
        }
    }
}

pub(super) struct SettingsView {
    pub(super) section: Section,
    pub(super) quick_controls_setup: bool,
    start_at_login: bool,
    start_at_login_pending: bool,
    discord_fields: Option<DiscordFields>,
    discord_saved: bool,
    widget_fields: Option<(Entity<TextInput>, Entity<TextInput>)>,
    launch_options_copy: LaunchOptionsCopy,
}

impl SettingsView {
    pub(super) fn new() -> Self {
        Self {
            section: Section::default(),
            quick_controls_setup: false,
            start_at_login: false,
            start_at_login_pending: false,
            discord_fields: None,
            discord_saved: false,
            widget_fields: None,
            launch_options_copy: LaunchOptionsCopy::Ready,
        }
    }
}

enum LaunchOptionsCopy {
    Ready,
    Copying,
    Copied,
    Failed(&'static str),
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
        if section == Section::General && !self.showing_settings(section) {
            match autostart::enabled() {
                Ok(Some(enabled)) => self.settings.start_at_login = enabled,
                Ok(None) => {}
                Err(error) => self.set_error(format!("{error:#}"), cx),
            }
        }
        self.home_assistant.dismiss_picker();
        self.settings.section = section;
        self.settings.quick_controls_setup = false;
        self.show_overlay(Overlay::Settings, window, cx);
    }

    pub(super) fn open_quick_controls_setup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.quick_controls_setup = true;
        self.show_overlay(Overlay::Settings, window, cx);
    }

    pub(super) fn reset_settings(&mut self) {
        self.settings.quick_controls_setup = false;
        self.home_assistant.close_settings();
        self.settings.widget_fields = None;
        if !matches!(
            self.settings.launch_options_copy,
            LaunchOptionsCopy::Copying
        ) {
            self.settings.launch_options_copy = LaunchOptionsCopy::Ready;
        }
    }

    pub(super) fn settings_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let section = self.settings.section;
        let setup = self.settings.quick_controls_setup;
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
                theme.bind(
                    theme
                        .button(name, active.then_some(accent))
                        .justify_start()
                        .h(r(29.))
                        .px(r(8.))
                        .child(icon(
                            symbol,
                            theme.color(if active { accent } else { MUTED }),
                            16.,
                        ))
                        .child(name),
                    move |window, cx| {
                        view.update(cx, |this, cx| this.open_settings(section, window, cx))
                    },
                )
            }));
        let body = if setup {
            self.quick_controls_setup(cx).into_any_element()
        } else {
            let content = match section {
                Section::General => self.general_settings(cx).into_any_element(),
                Section::Widgets => self.widget_settings(cx).into_any_element(),
                Section::Discord => self.discord_settings(cx).into_any_element(),
                Section::HomeAssistant => self.home_assistant_settings(cx).into_any_element(),
                Section::Integrations => self.integration_settings(theme).into_any_element(),
                _ => self.visibility_settings(section, cx).into_any_element(),
            };
            col()
                .id(("settings-content", section as usize))
                .flex_1()
                .overflow_y_scroll()
                .child(content)
                .into_any_element()
        };
        self.overlay_panel(
            Overlay::Settings,
            theme.title(if setup { "Quick controls" } else { "Settings" }),
            cx,
        )
        .when(!setup, |it| it.child(tabs))
        .child(body)
    }

    fn widget_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
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
        let save = theme.bind(
            theme
                .button("save-widget-settings", Some(TEAL))
                .child("Save locations"),
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
                                this.set_error(err.to_string(), cx);
                                cx.notify();
                            }
                        }
                    })
                }
            },
        );
        let weather_option = |id, text, toggle: fn(&mut Preferences)| {
            let view = cx.entity();
            theme.bind(theme.button(id, None).child(text), move |_, cx| {
                view.update(cx, |this, cx| {
                    let mut preferences = this.preferences.clone();
                    toggle(&mut preferences);
                    this.save_preferences(preferences, cx);
                })
            })
        };
        let copy_label = match self.settings.launch_options_copy {
            LaunchOptionsCopy::Copying => "Copying…",
            LaunchOptionsCopy::Copied => "Copied",
            _ => "Copy",
        };
        col()
            .gap(r(9.))
            .flex_none()
            .child(field(theme, "World clock time zones", zones))
            .child(theme.quiet("Separate IANA time zones with commas. Local time is always shown."))
            .child(field(theme, "Weather and air quality location", location))
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
                    .child(theme.label("Steam launch options for MangoHud FPS"))
                    .child(
                        theme
                            .button("copy-launch-options", None)
                            .h(r(25.))
                            .px(r(7.))
                            .child(copy_label)
                            .cursor_pointer()
                            .tab_index(0)
                            .focus(move |it| it.border_color(theme.color(TEAL)))
                            .on_click(cx.listener(Self::copy_launch_options)),
                    ),
            )
            .children(match self.settings.launch_options_copy {
                LaunchOptionsCopy::Failed(message) => {
                    Some(theme.quiet(message).text_color(theme.color(RED)))
                }
                _ => None,
            })
            .child(theme.quiet("Install MangoHud to track current FPS, 1% low, and 0.1% low."))
            .child(theme.quiet(FPS_LAUNCH_OPTIONS))
    }

    fn copy_launch_options(
        &mut self,
        event: &gpui::ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.settings.launch_options_copy,
            LaunchOptionsCopy::Copying
        ) {
            return;
        }
        // GPUI 0.2.2 uses the last key press serial for Wayland clipboard ownership.
        if cfg!(feature = "ui-gpui") && matches!(event, gpui::ClickEvent::Mouse(_)) {
            self.settings.launch_options_copy = LaunchOptionsCopy::Copying;
            cx.spawn(async move |this, cx| {
                let result = blocking::unblock(|| -> std::io::Result<_> {
                    let mut child = Command::new("wl-copy")
                        .arg("--type=text/plain")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .spawn()?;
                    let written = child
                        .stdin
                        .take()
                        .expect("wl-copy stdin was piped")
                        .write_all(FPS_LAUNCH_OPTIONS.as_bytes());
                    let status = child.wait()?;
                    written?;
                    Ok(status)
                })
                .await;
                let copy = match result {
                    Ok(status) if status.success() => LaunchOptionsCopy::Copied,
                    Err(error) if error.kind() == ErrorKind::NotFound => LaunchOptionsCopy::Failed(
                        "Install wl-clipboard for mouse support, or focus Copy with Tab and press Enter.",
                    ),
                    result => {
                        log::warn!("Could not copy Steam launch options with wl-copy: {result:?}");
                        LaunchOptionsCopy::Failed(
                            "Could not copy with wl-copy. Focus Copy with Tab and press Enter to retry.",
                        )
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.settings.launch_options_copy = if this.overlay.open == Some(Overlay::Settings) {
                        copy
                    } else {
                        LaunchOptionsCopy::Ready
                    };
                    cx.notify();
                });
            })
            .detach();
        } else {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                FPS_LAUNCH_OPTIONS.to_owned(),
            ));
            self.settings.launch_options_copy = LaunchOptionsCopy::Copied;
        }
        cx.notify();
    }

    fn general_settings(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let fullscreen_view = cx.entity();
        let autostart_view = cx.entity();
        let themes = row().flex_none().gap(r(5.)).children(
            [
                (ThemePreference::System, "System"),
                (ThemePreference::Light, "Light"),
                (ThemePreference::Dark, "Dark"),
            ]
            .map(|(preference, name)| {
                let active = self.preferences.theme == preference;
                let view = cx.entity();
                theme.bind(
                    theme
                        .button(name, active.then_some(TEAL))
                        .h(r(30.))
                        .px(r(9.))
                        .child(name),
                    move |window, cx| {
                        view.update(cx, |this, cx| this.set_theme(preference, window, cx))
                    },
                )
            }),
        );
        col()
            .gap(r(8.))
            .flex_none()
            .child(preference_row(
                theme,
                "theme-row",
                "Theme",
                "System follows your desktop appearance.",
                themes,
            ))
            .child(preference_row(
                theme,
                "start-at-login-row",
                "Start at login",
                if self.settings.start_at_login_pending {
                    "Waiting for the desktop to update the login launcher…"
                } else {
                    "Open Second Screen when you sign in."
                },
                theme.bind(
                    choice_button(
                        theme,
                        "startup-at-login",
                        self.settings.start_at_login,
                        "On",
                        "Off",
                    ),
                    move |_, cx| {
                        autostart_view.update(cx, |this, cx| {
                            this.toggle_start_at_login(cx);
                        });
                    },
                ),
            ))
            .child(preference_row(
                theme,
                "start-fullscreen-row",
                "Start fullscreen",
                "Applies the next time Second Screen opens.",
                theme.bind(
                    choice_button(
                        theme,
                        "startup-fullscreen",
                        self.preferences.start_fullscreen,
                        "On",
                        "Off",
                    ),
                    move |_, cx| {
                        fullscreen_view.update(cx, |this, cx| {
                            let mut preferences = this.preferences.clone();
                            preferences.start_fullscreen = !preferences.start_fullscreen;
                            this.save_preferences(preferences, cx);
                        });
                    },
                ),
            ))
            .child(self.launch_display_settings(theme, cx))
    }

    fn toggle_start_at_login(&mut self, cx: &mut Context<Self>) {
        if self.settings.start_at_login_pending {
            return;
        }
        let enabled = !self.settings.start_at_login;
        self.settings.start_at_login_pending = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = blocking::unblock(move || autostart::set_enabled(enabled)).await;
            let _ = this.update(cx, |this, cx| {
                this.settings.start_at_login_pending = false;
                match result {
                    Ok(()) => {
                        this.settings.start_at_login = enabled;
                        this.error = None;
                    }
                    Err(error) => this.set_error(format!("{error:#}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn launch_display_settings(&self, theme: Theme, cx: &Context<Self>) -> impl IntoElement {
        let saved = self.preferences.launch_display.as_deref();
        let supported = launch_display::supported();
        let (displays, note) = match &self.display.displays {
            _ if in_flatpak() => (
                &[][..],
                "Display selection requires a native package on Sway or KDE Plasma 6.".into(),
            ),
            _ if !supported => (
                &[][..],
                "Display selection is available on Sway and KDE Plasma 6.".into(),
            ),
            None => (&[][..], "Looking for displays…".into()),
            Some(Err(error)) => (&[][..], SharedString::from(error.clone())),
            Some(Ok(displays)) => {
                let note = match saved {
                    Some(name) if !displays.iter().any(|display| display.name == name) => {
                        format!("Waiting for {name} to reconnect.").into()
                    }
                    Some(name) => match &self.display.placement_error {
                        Some((output, error)) if output == name => {
                            format!("Retrying placement on {name}. {error}").into()
                        }
                        _ => "Moves back to this display when it reconnects.".into(),
                    },
                    None => "Let the desktop choose where the dashboard opens.".into(),
                };
                (displays.as_slice(), note)
            }
        };

        let disconnected = saved.filter(|name| !displays.iter().any(|d| d.name == *name));
        let choices = std::iter::once((None, "Desktop default", None))
            .chain(disconnected.map(|name| (Some(name.to_owned()), name, None)))
            .chain(displays.iter().map(|display| {
                (
                    Some(display.name.clone()),
                    display.name.as_str(),
                    Some(SharedString::from(display.description.clone())),
                )
            }));
        let buttons = row()
            .justify_end()
            .max_w(gpui::relative(0.72))
            .gap(r(5.))
            .flex_wrap()
            .when(supported || saved.is_some(), |row| {
                row.children(choices.map(|(name, label, detail)| {
                    let selected = name.as_deref() == saved;
                    let view = cx.entity();
                    let label = SharedString::from(label.to_owned());
                    let button = theme
                        .button(label.clone(), selected.then_some(TEAL))
                        .h(r(30.))
                        .px(r(9.))
                        .child(label)
                        .when_some(detail, |button, detail| {
                            button.tooltip(move |_, cx| cx.new(|_| Tooltip(detail.clone())).into())
                        });
                    theme.bind(button, move |_, cx| {
                        view.update(cx, |this, cx| {
                            if this.preferences.launch_display != name {
                                let mut preferences = this.preferences.clone();
                                preferences.launch_display = name.clone();
                                this.save_preferences(preferences, cx);
                            }
                        });
                    })
                }))
            });
        preference_row(theme, "launch-display-row", "Launch display", note, buttons)
    }

    fn visibility_settings(&self, section: Section, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
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
            Section::Notifications => (
                "Notifications",
                "Choose which apps appear on the dashboard.",
                "Apps appear here as they send desktop notifications.",
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
                self.storage
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
            Section::Notifications => (
                &self.preferences.hidden_notification_apps,
                self.utilities
                    .notification_apps
                    .iter()
                    .map(|(key, name)| {
                        (
                            key.clone(),
                            (
                                name.clone(),
                                key.strip_prefix("desktop:").unwrap_or("").to_owned(),
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
                    detail: if section == Section::Notifications {
                        "Saved filter".into()
                    } else {
                        "Unavailable".into()
                    },
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
            .child(theme.title(title_text))
            .child(theme.quiet(hint))
            .when(section == Section::Notifications, |it| {
                let view = cx.entity();
                it.child(preference_row(
                    theme,
                    "notification-bodies-row",
                    "Message bodies",
                    "Show only the app, title, and time when bodies are hidden.",
                    theme.bind(
                        choice_button(
                            theme,
                            "notification-bodies",
                            !self.preferences.hide_notification_bodies,
                            "Shown",
                            "Hidden",
                        ),
                        move |_, cx| {
                            view.update(cx, |this, cx| {
                                let mut preferences = this.preferences.clone();
                                preferences.hide_notification_bodies =
                                    !preferences.hide_notification_bodies;
                                this.save_preferences(preferences, cx);
                            });
                        },
                    ),
                ))
                .child(theme.quiet(self.notification_status_message()))
            })
            .when(choices.is_empty(), |it| {
                it.child(theme.quiet(empty).py(r(10.)))
            })
            .children(choices.into_iter().map(|(key, choice)| {
                let view = cx.entity();
                let name = choice.name;
                let saved_name = name.clone();
                let id = SharedString::from(key.clone());
                preference_row(
                    theme,
                    id.clone(),
                    name,
                    choice.detail,
                    theme.bind(
                        choice_button(theme, id, !choice.hidden, "Shown", "Hidden"),
                        move |_, cx| {
                            view.update(cx, |this, cx| {
                                let mut preferences = this.preferences.clone();
                                let hidden = section.hidden(&mut preferences);
                                if hidden.remove(&key).is_none() {
                                    hidden.insert(key.clone(), saved_name.clone());
                                }
                                if this.save_preferences(preferences, cx)
                                    && section == Section::Notifications
                                {
                                    this.apply_notification_filters();
                                }
                            });
                        },
                    ),
                )
            }))
    }

    fn discord_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
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
        let save = theme.bind(
            theme
                .button("save-discord-settings", Some(VIOLET))
                .child("Save credentials"),
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
                            this.set_error(error.to_string(), cx);
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
            .child(theme.title("Discord voice RPC"))
            .child(theme.quiet(note))
            .child(field(theme, "Application ID", id))
            .child(
                split()
                    .gap(r(10.))
                    .child(field(theme, "Client secret", secret))
                    .child(field(theme, "Access token", token)),
            )
            .child(theme.quiet(format!(
                "For client-secret login, add {DISCORD_REDIRECT_URI} under OAuth2 → Redirects in the Discord Developer Portal."
            )))
            .child(
                row()
                    .gap(r(7.))
                    .flex_wrap()
                    .child(save)
                    .when(saved || using_environment, |row| {
                        row.child(theme.bind(
                            theme.button("clear-discord-settings", None).child("Clear credentials"),
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
                        row.child(theme.bind(
                            theme.button("use-discord-environment", None).child("Use environment"),
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
            self.focus_overlay(window, cx);
            self.settings.discord_fields = None;
            self.settings.discord_saved =
                credentials.as_ref().is_some_and(DiscordCredentials::valid);
            if changed {
                self.send_discord(DiscordAction::Configure(credentials));
            }
        }
    }

    pub(super) fn save_preferences(
        &mut self,
        preferences: Preferences,
        cx: &mut Context<Self>,
    ) -> bool {
        let saved = match preferences.save() {
            Ok(()) => {
                self.preferences = preferences;
                self.error = None;
                true
            }
            Err(error) => {
                self.set_error(format!("{error:#}"), cx);
                false
            }
        };
        cx.notify();
        saved
    }

    pub(super) fn set_theme(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.theme != preference {
            let mut preferences = self.preferences.clone();
            preferences.theme = preference;
            if self.save_preferences(preferences, cx) {
                Theme::apply(preference, window.appearance(), cx);
            }
        }
    }
}

fn preference_row(
    theme: Theme,
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
        .border_color(theme.color(STROKE))
        .bg(theme.color(BACK).opacity(0.44))
        .child(
            col()
                .flex_1()
                .gap(r(3.))
                .child(
                    theme
                        .label(name.clone())
                        .id("preference-label")
                        .text_color(theme.color(INK))
                        .truncate()
                        .tooltip(move |_, cx| cx.new(|_| Tooltip(name.clone())).into()),
                )
                .child(theme.quiet(detail.into())),
        )
        .child(control)
}

fn choice_button(
    theme: Theme,
    id: impl Into<gpui::ElementId>,
    enabled: bool,
    on: &str,
    off: &str,
) -> gpui::Stateful<gpui::Div> {
    theme
        .button(id, enabled.then_some(TEAL))
        .w(r(84.))
        .h(r(30.))
        .px(r(6.))
        .child(div().child(if enabled {
            on.to_owned()
        } else {
            off.to_owned()
        }))
}
