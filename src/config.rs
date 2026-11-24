use std::{
    collections::BTreeMap,
    env, fs,
    io::{ErrorKind, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub theme: ThemePreference,
    pub start_fullscreen: bool,
    pub launch_display: Option<String>,
    pub clock_zones: Option<Vec<String>>,
    pub weather_location: String,
    pub fahrenheit: bool,
    pub weather_mph: bool,
    pub weather_12_hour: bool,
    pub hidden_media: BTreeMap<String, String>,
    pub hidden_audio: BTreeMap<String, String>,
    pub hidden_drives: BTreeMap<String, String>,
    pub hidden_notification_apps: BTreeMap<String, String>,
    pub hide_notification_bodies: bool,
    pub discord: Option<DiscordCredentials>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_assistant: Option<HomeAssistantCredentials>,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl Preferences {
    pub fn load() -> Self {
        let result = path("settings.json").as_deref().map(read_json).transpose();
        match result {
            Ok(preferences) => preferences.unwrap_or_default(),
            Err(error) => {
                log::warn!("Could not load Settings: {error}");
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = path("settings.json").context("No settings directory is available.")?;
        write_json(&path, self).context("Could not save Settings")
    }
}

pub const DISCORD_REDIRECT_URI: &str = "http://127.0.0.1";

#[derive(Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct DiscordCredentials {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

impl DiscordCredentials {
    pub fn valid(&self) -> bool {
        !self.client_id.is_empty()
            && self.client_id.bytes().all(|byte| byte.is_ascii_digit())
            && (self.secret.is_some() || self.access_token.is_some())
            && self.secret.as_deref().is_none_or(valid_credential)
            && self.access_token.as_deref().is_none_or(valid_credential)
    }

    pub fn from_env() -> Option<Self> {
        let credentials = Self {
            client_id: setting("SECONDSCREEN_DISCORD_CLIENT_ID").unwrap_or_default(),
            secret: setting("SECONDSCREEN_DISCORD_CLIENT_SECRET"),
            access_token: setting("SECONDSCREEN_DISCORD_ACCESS_TOKEN"),
        };
        credentials.valid().then_some(credentials)
    }

    pub fn from_input(client_id: &str, secret: &str, access_token: &str) -> Result<Self> {
        let credentials = Self {
            client_id: client_id.trim().to_owned(),
            secret: nonempty(secret),
            access_token: nonempty(access_token),
        };
        ensure!(
            credentials.valid(),
            "Enter a numeric application ID and a client secret or access token."
        );
        Ok(credentials)
    }
}

fn valid_credential(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
pub struct HomeAssistantCredentials {
    pub url: String,
    pub token: String,
}

impl HomeAssistantCredentials {
    pub fn from_input(address: &str, token: &str) -> Result<Self> {
        let address = address.trim();
        let mut url = url::Url::parse(address)
            .ok()
            .filter(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.host().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
            })
            .context("Enter the Home Assistant base URL, including http:// or https://.")?;
        if url.path().ends_with('/') {
            url.path_segments_mut()
                .expect("HTTP URLs have path segments")
                .pop_if_empty();
        }
        let token = token.trim();
        ensure!(
            valid_credential(token),
            "Enter a Home Assistant long-lived access token."
        );
        Ok(Self {
            url: url.into(),
            token: token.to_owned(),
        })
    }
}

fn setting(name: &str) -> Option<String> {
    nonempty(&env::var(name).ok()?)
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub fn config_home() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|path| Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| Path::new(path).is_absolute())
                .map(|home| PathBuf::from(home).join(".config"))
        })
}

pub fn path(file: &str) -> Option<PathBuf> {
    config_home().map(|directory| directory.join("secondscreen").join(file))
}

pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid configuration file")),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_file(path, |file| {
        serde_json::to_writer_pretty(&mut *file, value)?;
        file.write_all(b"\n")?;
        Ok(())
    })
}

pub fn write_file(path: &Path, write: impl FnOnce(&mut fs::File) -> Result<()>) -> Result<()> {
    static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);
    let directory = path
        .parent()
        .context("No configuration directory is available.")?;
    fs::create_dir_all(directory)?;
    let staging = directory.join(format!(
        ".save-{}-{}.tmp",
        std::process::id(),
        NEXT_SAVE.fetch_add(1, Ordering::Relaxed),
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)?;
    let result = (|| -> Result<()> {
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&staging, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result
}
