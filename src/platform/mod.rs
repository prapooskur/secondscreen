#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{autostart, calendar, desktop_apps, launch_display, start, touch};

pub const APP_ID: &str = "io.github.secondscreen";

#[cfg(target_os = "linux")]
pub fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

use async_channel::Sender;

use crate::model::{AudioAction, DiscordAction, MediaAction};

pub struct Controls {
    pub storage: std::sync::mpsc::Sender<bool>,
    pub audio: Sender<AudioAction>,
    pub media: Sender<(String, MediaAction)>,
    pub discord: Sender<DiscordAction>,
}
