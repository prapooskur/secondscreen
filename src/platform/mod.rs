#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{autostart, launch_display, start};

pub const APP_ID: &str = "io.github.secondscreen";

use std::sync::mpsc::Sender;

use crate::model::{AudioAction, DiscordAction, MediaAction};

pub struct Controls {
    pub audio: Sender<AudioAction>,
    pub media: Sender<(String, MediaAction)>,
    pub discord: Sender<DiscordAction>,
}
