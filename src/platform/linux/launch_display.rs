mod kde;
mod sway;
mod wayland;

use std::{env, time::Duration};

use anyhow::{Context, Result};

use crate::platform::in_flatpak;

pub use wayland::Displays;

const RETRY_INTERVAL: Duration = Duration::from_millis(75);
const WINDOW_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub description: String,
}

impl Display {
    fn with_size(id: u32, name: String, make: &str, model: &str, width: u32, height: u32) -> Self {
        let mut description = [make, model]
            .into_iter()
            .filter(|value| !value.is_empty() && *value != "Unknown")
            .collect::<Vec<_>>()
            .join(" ");
        if width > 0 && height > 0 {
            if !description.is_empty() {
                description.push_str(" · ");
            }
            description.push_str(&format!("{width} × {height}"));
        }
        Self {
            id,
            name,
            description,
        }
    }
}

enum Desktop {
    Sway,
    Kde,
}

impl Desktop {
    fn current() -> Option<Self> {
        if in_flatpak() {
            return None;
        }
        // Recognize the desktop first in case a nested session inherits SWAYSOCK.
        let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        for desktop in desktop.split(':') {
            if desktop.eq_ignore_ascii_case("kde")
                && env::var("KDE_SESSION_VERSION").as_deref() != Ok("5")
            {
                return Some(Self::Kde);
            }
            if desktop.eq_ignore_ascii_case("gnome") {
                return None;
            }
        }
        env::var_os("SWAYSOCK")
            .is_some_and(|socket| !socket.is_empty())
            .then_some(Self::Sway)
    }
}

pub fn supported() -> bool {
    Desktop::current().is_some()
}

pub fn prepare_startup(display: &str, title: &str) -> Result<Option<String>> {
    let Some(desktop) = Desktop::current() else {
        return Ok(None);
    };
    // Leave an inherited, single-use Wayland connection for GPUI.
    if env::var_os("WAYLAND_SOCKET").is_some() {
        return Ok(None);
    }
    if !Displays::open()?
        .scan()?
        .iter()
        .any(|output| output.name == display)
    {
        return Ok(None);
    }
    if let Desktop::Sway = desktop {
        match sway::startup_title(display) {
            Ok(title) => return Ok(Some(title)),
            Err(error) => log::debug!("Could not prepare Sway launch placement: {error:#}"),
        }
    }
    Ok(Some(title.into()))
}

pub fn place_window(display: &str, fullscreen: bool) -> Result<()> {
    match Desktop::current().context("Display placement is unavailable on this desktop")? {
        Desktop::Sway => sway::place_window(display, fullscreen),
        Desktop::Kde => kde::place_window(display, fullscreen),
    }
}
