mod mangohud;

use anyhow::{Context, Result, ensure};
use rustix::fs::{CWD, Mode, OFlags, mkfifoat, open};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Instant,
};

use crate::platform::in_flatpak;

const MANGOHUD_DIR: &str = "secondscreen-mangohud";

pub(in crate::ui) fn launch_options(directory: Option<&str>) -> String {
    let config = match directory {
        Some(directory) => {
            let mut config = String::from("'autostart_log=1,output_folder=");
            for ch in directory.chars() {
                match ch {
                    // Escape MangoHud's config delimiters inside the shell-quoted value.
                    ',' | ':' | ';' | '=' => {
                        config.push('\\');
                        config.push(ch);
                    }
                    '\'' => config.push_str("'\\''"),
                    _ => config.push(ch),
                }
            }
            config.push('\'');
            config
        }
        None => format!(r#""autostart_log=1,output_folder=$XDG_RUNTIME_DIR/{MANGOHUD_DIR}""#),
    };
    format!("MANGOHUD_CONFIG={config} mangohud %command%")
}

fn runtime_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is unavailable.")
}

pub(in crate::ui) fn prepare_mangohud(directory: Option<&str>) -> Result<()> {
    if let Some(directory) = directory {
        ensure!(
            Path::new(directory).is_absolute(),
            "Enter an absolute folder path."
        );
        // MangoHud parses each value into a 256-byte buffer including the terminator.
        ensure!(
            directory.len() <= 255 && !directory.chars().any(char::is_control),
            "MangoHud folder paths must be at most 255 bytes and contain no control characters."
        );
        fs::read_dir(directory).with_context(|| format!("Could not open {directory}"))?;
        return Ok(());
    }
    let path = runtime_dir()?.join(MANGOHUD_DIR);
    match fs::create_dir(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Could not create {}", path.display())),
    }
}

pub(super) struct Setup {
    pub mangohud: bool,
    pub gamescope: bool,
    pub logging_error: Option<String>,
}

impl Setup {
    pub fn check(directory: Option<&str>) -> Self {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let available = |name| {
            std::env::split_paths(&path).any(|dir| {
                fs::metadata(dir.join(name)).is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
            })
        };
        Self {
            mangohud: available("mangohud"),
            gamescope: available("gamescope"),
            logging_error: prepare_mangohud(directory)
                .err()
                .map(|error| format!("{error:#}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    MangoHud(u64),
    Gamescope,
}

pub(super) struct Reading {
    pub source: Source,
    pub current: f32,
    pub low_1: Option<f32>,
    pub low_01: Option<f32>,
    pub frame_time_ms: Option<f32>,
    pub peak_frame_time_ms: Option<f32>,
}

pub(super) struct Feed {
    mango: mangohud::Feed,
    default_mango_directory: PathBuf,
    gamescope: Option<Gamescope>,
    gamescope_path: PathBuf,
}

impl Feed {
    pub fn open() -> Result<Self> {
        let dir = runtime_dir()?;
        // Flatpak can share a directory containing a FIFO, but cannot mount the FIFO itself.
        let gamescope_pipe = if in_flatpak() {
            "secondscreen-gamescope/secondscreen-fps"
        } else {
            "secondscreen-fps"
        };
        Ok(Self {
            mango: mangohud::Feed::default(),
            default_mango_directory: dir.join(MANGOHUD_DIR),
            gamescope: None,
            gamescope_path: dir.join(gamescope_pipe),
        })
    }

    pub fn sample(&mut self, directory: Option<&str>) -> Result<Option<Reading>> {
        let directory = directory.map_or(self.default_mango_directory.as_path(), Path::new);
        let mango = self.mango.sample(directory);
        // Drain the FIFO even when MangoHud is active so a Gamescope writer can keep running.
        let gamescope = self.sample_gamescope();
        match (mango, gamescope) {
            (Ok(Some(reading)), _) => Ok(Some(reading)),
            (_, Ok(Some(current))) => Ok(Some(Reading {
                source: Source::Gamescope,
                current,
                low_1: None,
                low_01: None,
                frame_time_ms: None,
                peak_frame_time_ms: None,
            })),
            (mango, gamescope) => {
                mango.context("MangoHud")?;
                gamescope.context("Gamescope")?;
                Ok(None)
            }
        }
    }

    fn sample_gamescope(&mut self) -> Result<Option<f32>> {
        if self.gamescope.is_none() {
            self.gamescope = Some(Gamescope::at(&self.gamescope_path)?);
        }
        let sample = self.gamescope.as_mut().unwrap().sample();
        if sample.is_err() {
            self.gamescope = None;
        }
        Ok(sample?)
    }
}

struct Gamescope {
    pipe: File,
    pending: Vec<u8>,
    last: Option<(f32, Instant)>,
}

impl Gamescope {
    fn at(path: &Path) -> Result<Self> {
        match mkfifoat(CWD, path, Mode::RUSR | Mode::WUSR) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(e) => return Err(e.into()),
        }
        let pipe = File::from(open(
            path,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?);
        ensure!(
            pipe.metadata()?.file_type().is_fifo(),
            "The Gamescope stats path is not a FIFO."
        );
        Ok(Self {
            pipe,
            pending: Vec::new(),
            last: None,
        })
    }

    pub fn sample(&mut self) -> std::io::Result<Option<f32>> {
        let mut bytes = [0u8; 4096];
        // A Gamescope record is a short newline-terminated key=value pair.
        for _ in 0..8 {
            match self.pipe.read(&mut bytes) {
                Ok(0) => {
                    self.last = None;
                    self.pending.clear();
                    break;
                }
                Ok(n) => {
                    self.pending.extend_from_slice(&bytes[..n]);
                    if let Some(end) = self.pending.iter().rposition(|&b| b == b'\n') {
                        for line in self.pending[..end].split(|&b| b == b'\n') {
                            if let Some(fps) = std::str::from_utf8(line)
                                .ok()
                                .and_then(|s| s.strip_prefix("fps="))
                                .and_then(|s| s.parse::<f32>().ok())
                                .filter(|n| n.is_finite() && *n >= 0.)
                            {
                                self.last = Some((fps, Instant::now()));
                            }
                        }
                        self.pending.drain(..=end);
                    }
                    if self.pending.len() > 4096 {
                        self.pending.clear();
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        // Gamescope emits a sample every 300 frames. Allow two sample periods.
        Ok(self
            .last
            .filter(|(fps, at)| at.elapsed().as_secs_f32() < (600. / fps.max(1.)).max(10.))
            .map(|(fps, _)| fps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, process::Command};

    #[test]
    fn preserves_default_launch_options_and_quotes_custom_paths() {
        assert_eq!(
            launch_options(None),
            r#"MANGOHUD_CONFIG="autostart_log=1,output_folder=$XDG_RUNTIME_DIR/secondscreen-mangohud" mangohud %command%"#,
        );

        let command = launch_options(Some("/home/player/FPS logs/$HOME's,x:y;z=q"));
        let script =
            format!("mangohud() {{ printf '%s\\n%s\\n' \"$MANGOHUD_CONFIG\" \"$1\"; }}\n{command}");
        let output = Command::new("sh").arg("-c").arg(script).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "autostart_log=1,output_folder=/home/player/FPS logs/$HOME's\\,x\\:y\\;z\\=q\n%command%\n",
        );
    }

    #[test]
    fn switches_mangohud_log_directories_and_restores_default() {
        let root = tempfile::tempdir().unwrap();
        let default = root.path().join(MANGOHUD_DIR);
        let custom = root.path().join("FPS logs");
        fs::create_dir(&default).unwrap();
        fs::create_dir(&custom).unwrap();
        fs::write(default.join("game.csv"), "100,10,10000000\n").unwrap();
        fs::write(custom.join("game.csv"), "50,20,20000000\n").unwrap();
        let mut feed = Feed {
            mango: mangohud::Feed::default(),
            default_mango_directory: default,
            gamescope: None,
            gamescope_path: root.path().join("secondscreen-fps"),
        };

        let original = feed.sample(None).unwrap().unwrap();
        assert_eq!(original.current, 100.);
        let selected = feed.sample(custom.to_str()).unwrap().unwrap();
        assert_eq!(selected.current, 50.);
        assert_ne!(selected.source, original.source);
        let restored = feed.sample(None).unwrap().unwrap();
        assert_eq!(restored.current, 100.);
        assert_ne!(restored.source, selected.source);
    }

    #[test]
    fn reads_fragmented_gamescope_records_and_detects_disconnect() {
        let path = std::env::temp_dir().join(format!("ss-fps-test-{}", std::process::id()));
        let mut feed = Gamescope::at(&path).unwrap();
        assert_eq!(feed.sample().unwrap(), None);
        let mut writer = File::options().write(true).open(&path).unwrap();
        writer.write_all(b"focus=42\nfps=11").unwrap();
        assert_eq!(feed.sample().unwrap(), None);
        writer.write_all(b"9.8\nfps=NaN\n").unwrap();
        assert_eq!(feed.sample().unwrap(), Some(119.8));
        drop(writer);
        assert_eq!(feed.sample().unwrap(), None);
        std::fs::remove_file(path).unwrap();
    }
}
