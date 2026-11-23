use std::{
    env, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{config, platform::APP_ID};

fn entry_path() -> Result<PathBuf> {
    let directory =
        config::config_home().context("No user configuration directory is available.")?;
    Ok(directory
        .join("autostart")
        .join(format!("{APP_ID}.desktop")))
}

pub fn enabled() -> Result<bool> {
    let entry = match fs::read_to_string(entry_path()?) {
        Ok(entry) => entry,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
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
    Ok(!hidden)
}

pub fn set_enabled(enabled: bool) -> Result<()> {
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
