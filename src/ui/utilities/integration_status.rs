use gpui::{IntoElement, Styled, prelude::*};

use crate::{platform::calendar::Status as CalendarStatus, ui::theme::*};

use super::{Dashboard, fps, notifications};

pub(super) type Status = (&'static str, String, Color);

impl Dashboard {
    pub(in crate::ui) fn integration_settings(&self, theme: Theme) -> impl IntoElement {
        let calendar = match self.utilities.calendar.status() {
            Some(CalendarStatus::Ready) => (
                "Connected",
                "Reading events from your selected system calendars.",
                TEAL,
            ),
            Some(CalendarStatus::Partial) => (
                "Partially connected",
                "Open GNOME Calendar or Evolution to check the calendars that failed to load.",
                AMBER,
            ),
            Some(CalendarStatus::NoSelectedCalendars) => (
                "No calendars selected",
                "Enable at least one calendar in GNOME Calendar or Evolution.",
                AMBER,
            ),
            Some(CalendarStatus::CalendarsUnavailable) => (
                "Calendars unavailable",
                "Open GNOME Calendar or Evolution and check that your calendars load there.",
                RED,
            ),
            Some(CalendarStatus::ServiceUnavailable) => (
                "Service unavailable",
                "Install or check Evolution Data Server on the host, then open GNOME Calendar or Evolution.",
                RED,
            ),
            None => ("Checking", "Connecting to Evolution Data Server…", MUTED),
        };
        let notifications = match self.utilities.notification_status {
            notifications::Status::Connecting => {
                ("Checking", "Connecting to the desktop session bus…", MUTED)
            }
            notifications::Status::Listening => (
                "Listening",
                "Send a desktop notification to check the feed.",
                TEAL,
            ),
            notifications::Status::Blocked if in_flatpak() => (
                "Blocked by the sandbox",
                "This Flatpak cannot monitor host notifications. Use a native package for this widget.",
                AMBER,
            ),
            notifications::Status::Blocked => (
                "Monitoring unavailable",
                "This desktop session bus blocks or does not support notification monitoring.",
                AMBER,
            ),
            notifications::Status::Unavailable => (
                "Feed unavailable",
                "Run Second Screen within your desktop session. The app will retry automatically.",
                RED,
            ),
            notifications::Status::Disconnected => (
                "Disconnected",
                "Reconnecting to the desktop session bus…",
                AMBER,
            ),
        };
        let (fps_status, fps_detail, fps_color) = self.fps_integration_status();
        col().gap(r(9.)).flex_none().children([
            status_row(theme, "Calendar", calendar.0, calendar.1, calendar.2).into_any_element(),
            status_row(theme, "FPS", fps_status, fps_detail, fps_color).into_any_element(),
            status_row(
                theme,
                "Notifications",
                notifications.0,
                notifications.1,
                notifications.2,
            )
            .into_any_element(),
        ])
    }

    pub(super) fn fps_integration_status(&self) -> Status {
        let state = &self.utilities;
        if let Some(reading) = &state.fps_reading {
            let detail = match reading.source {
                fps::Source::MangoHud(_) => "Receiving per-frame data from MangoHud.",
                fps::Source::Gamescope => "Receiving average FPS from Gamescope.",
            };
            return ("Receiving data", detail.into(), TEAL);
        }
        if let Some(error) = &state.fps_error {
            return ("Feed unavailable", error.clone(), RED);
        }
        let Some(setup) = &state.fps_setup else {
            return ("Checking", "Looking for a game feed…".into(), MUTED);
        };
        if let Some(error) = &setup.logging_error {
            return ("Logging unavailable", error.clone(), RED);
        }
        if in_flatpak() {
            return (
                "Waiting for data",
                "Host games may write to a runtime directory this Flatpak cannot see. Use a native package if the host feed stays blank.".into(),
                AMBER,
            );
        }
        if !setup.mangohud && !setup.gamescope {
            return (
                "No FPS tools found in PATH",
                "Install MangoHud for your game’s runtime and copy its launch options from Settings → Widgets.".into(),
                AMBER,
            );
        }
        let detail = if setup.mangohud {
            "Start a game with the MangoHud launch options from Settings → Widgets."
        } else {
            "Start a game with Gamescope’s stats output pointed at $XDG_RUNTIME_DIR/secondscreen-fps."
        };
        ("Waiting for a game", detail.into(), MUTED)
    }
}

fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

fn status_row(
    theme: Theme,
    name: &'static str,
    status: &'static str,
    detail: impl Into<gpui::SharedString>,
    color: Color,
) -> impl IntoElement {
    col()
        .gap(r(4.))
        .p(r(8.))
        .rounded(r(8.))
        .border_1()
        .border_color(theme.color(STROKE))
        .bg(theme.color(BACK).opacity(0.44))
        .child(
            row()
                .gap(r(8.))
                .justify_between()
                .child(theme.label(name))
                .child(theme.quiet(status).text_color(theme.color(color))),
        )
        .child(theme.quiet(detail.into()))
}
