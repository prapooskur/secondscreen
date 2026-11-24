use std::{
    env, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use ashpd::desktop::background::Background;

use crate::{
    config,
    platform::{APP_ID, in_flatpak},
};

fn entry_path() -> Result<PathBuf> {
    let directory = if in_flatpak() {
        config::xdg_home("HOST_XDG_CONFIG_HOME", ".config")
    } else {
        config::config_home()
    }
    .context("No user configuration directory is available.")?;
    Ok(directory
        .join("autostart")
        .join(format!("{APP_ID}.desktop")))
}

pub fn enabled() -> Result<Option<bool>> {
    let path = entry_path()?;
    let entry = match fs::read_to_string(&path) {
        Ok(entry) => entry,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            // A custom host directory created by the portal may only be visible after relaunch.
            if in_flatpak() && !path.parent().is_some_and(Path::is_dir) {
                return Ok(None);
            }
            return Ok(Some(false));
        }
        Err(error) => return Err(error).context("Could not read the login launcher"),
    };
    let hidden = entry
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != "[Desktop Entry]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter_map(|line| line.split_once('='))
        .any(|(key, value)| key.trim() == "Hidden" && value.trim() == "true");
    Ok(Some(!hidden))
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    if in_flatpak() {
        return futures_lite::future::block_on(set_flatpak_enabled(enabled));
    }
    let path = entry_path()?;
    if enabled {
        let executable = env::current_exe().context("Could not locate Second Screen")?;
        let exec = desktop_exec(&executable)?;
        config::write_file(&path, |file| {
            writeln!(
                file,
                "[Desktop Entry]\nType=Application\nName=Second Screen\nExec=env {exec}\nTerminal=false"
            )?;
            Ok(())
        })
        .context("Could not create the login launcher")
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("Could not remove the login launcher"),
        }
    }
}

async fn set_flatpak_enabled(enabled: bool) -> Result<()> {
    let request = Background::request()
        .reason("Open Second Screen when you sign in.")
        .auto_start(enabled)
        .send()
        .await
        .context("Could not reach the desktop's Background portal")?;
    let response = request
        .response()
        .context("The desktop declined the login launcher request")?;
    ensure!(
        response.auto_start() == enabled,
        "The desktop did not update the login launcher"
    );
    Ok(())
}

fn desktop_exec(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .filter(|path| !path.chars().any(char::is_control))
        .context("The executable path cannot be written to a login launcher.")?;
    // Exec quoting is applied after the desktop file's string escaping.
    let mut quoted = String::from("\"");
    for character in path.chars() {
        match character {
            '\\' | '"' | '`' | '$' => quoted.push('\\'),
            '%' => quoted.push('%'),
            _ => {}
        }
        quoted.push(character);
    }
    quoted.push('"');
    Ok(quoted.replace('\\', "\\\\"))
}
