use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

/// Resolve stream metadata on the audio worker, keeping filesystem work off the UI thread.
pub(super) struct AppIcons {
    theme: String,
    desktop_icons: BTreeMap<String, String>,
    cache: BTreeMap<Vec<String>, Option<PathBuf>>,
}

impl Default for AppIcons {
    fn default() -> Self {
        let mut roots = Vec::new();
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            roots.push(
                env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".local/share")),
            );
            roots.push(home.join(".local/share/flatpak/exports/share"));
        }
        roots.extend(env::split_paths(
            &env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
        ));
        roots.push("/var/lib/flatpak/exports/share".into());
        let mut desktop_icons = BTreeMap::new();
        for root in roots {
            let Ok(entries) = fs::read_dir(root.join("applications")) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "desktop")
                    && let Ok(contents) = fs::read_to_string(&path)
                {
                    index_desktop(&path, &contents, &mut desktop_icons);
                }
            }
        }
        Self {
            theme: freedesktop_icons::default_theme_gtk().unwrap_or_else(|| "hicolor".into()),
            desktop_icons,
            cache: BTreeMap::new(),
        }
    }
}

impl AppIcons {
    pub(super) fn resolve(&mut self, properties: &BTreeMap<String, String>) -> Option<PathBuf> {
        let names: Vec<_> = [
            "application.icon_name",
            "application.desktop",
            "application.id",
            "application.process.binary",
            "application.name",
        ]
        .into_iter()
        .filter_map(|key| {
            let value = properties.get(key).filter(|value| !value.is_empty())?;
            if key == "application.process.binary" {
                Path::new(value).file_name()?.to_str().map(str::to_owned)
            } else {
                Some(value.clone())
            }
        })
        .collect();
        if let Some(icon) = self.cache.get(&names) {
            return icon.clone();
        }
        let icon = names.iter().find_map(|name| {
            let key = name.trim_end_matches(".desktop").to_lowercase();
            self.desktop_icons
                .get(&key)
                .and_then(|icon| lookup(icon, &self.theme))
                .or_else(|| lookup(name, &self.theme))
                .or_else(|| lookup(&key, &self.theme))
        });
        self.cache.insert(names, icon.clone());
        icon
    }
}

pub(super) fn lookup(name: &str, theme: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_owned());
    }
    freedesktop_icons::lookup(name)
        .with_theme(theme)
        .with_size(48)
        .with_cache()
        .find()
}

fn index_desktop(path: &Path, contents: &str, icons: &mut BTreeMap<String, String>) {
    let mut in_entry = false;
    let mut values = BTreeMap::new();
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((key, value)) = line.split_once('=') {
            values.insert(key, value);
        }
    }
    let Some(icon) = values.get("Icon").filter(|icon| !icon.is_empty()) else {
        return;
    };
    let id = path.file_stem().and_then(|id| id.to_str());
    let executable = values
        .get("Exec")
        .and_then(|exec| exec.split_whitespace().next())
        .and_then(|exec| Path::new(exec).file_name())
        .and_then(|exec| exec.to_str());
    for key in [
        id,
        values.get("Name").copied(),
        values.get("StartupWMClass").copied(),
        executable,
    ]
    .into_iter()
    .flatten()
    {
        icons
            .entry(key.to_lowercase())
            .or_insert_with(|| (*icon).to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_icon_matches_stream_identifiers_and_ignores_action_icons() {
        let mut icons = BTreeMap::new();
        index_desktop(
            Path::new("org.example.Player.desktop"),
            "[Desktop Entry]\nName=Example Player\nExec=/usr/bin/player %U\nStartupWMClass=PlayerWindow\nIcon=example-player\n[Desktop Action Open]\nIcon=document-open\n",
            &mut icons,
        );
        for key in [
            "org.example.player",
            "example player",
            "player",
            "playerwindow",
        ] {
            assert_eq!(icons.get(key).map(String::as_str), Some("example-player"));
        }
        // A system entry must not replace an icon from a user entry indexed first.
        index_desktop(
            Path::new("org.example.Player.desktop"),
            "[Desktop Entry]\nIcon=system-player\n",
            &mut icons,
        );
        assert_eq!(icons["org.example.player"], "example-player");
    }
}
