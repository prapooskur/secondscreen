mod kde;
mod sway;
mod wayland;

use std::{env, time::Duration};

use anyhow::Result;

const RETRY_INTERVAL: Duration = Duration::from_millis(75);
const WINDOW_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Display {
    pub name: String,
    pub description: String,
}

impl Display {
    fn with_size(name: String, make: &str, model: &str, width: u32, height: u32) -> Self {
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
        Self { name, description }
    }
}

enum Desktop {
    Sway,
    Kde,
}

impl Desktop {
    fn current() -> Option<Self> {
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

    fn displays(&self) -> Result<Vec<Display>> {
        match self {
            Self::Sway => sway::connected(),
            Self::Kde => wayland::connected(),
        }
    }
}

pub fn supported() -> bool {
    Desktop::current().is_some()
}

pub fn connected() -> Result<Vec<Display>> {
    let Some(desktop) = Desktop::current() else {
        return Ok(Vec::new());
    };
    let mut displays = desktop.displays()?;
    displays.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    Ok(displays)
}

pub fn place_window(display: &str) -> Result<()> {
    let Some(desktop) = Desktop::current() else {
        return Ok(());
    };
    let displays = desktop.displays()?;
    if !displays.iter().any(|output| output.name == display) {
        log::debug!(
            "Launch display {display} is unavailable. Connected outputs: {:?}",
            displays
                .iter()
                .map(|output| &output.name)
                .collect::<Vec<_>>()
        );
        return Ok(());
    }
    match desktop {
        Desktop::Sway => sway::place_window(display),
        Desktop::Kde => kde::place_window(display),
    }
}
