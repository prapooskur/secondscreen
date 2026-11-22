mod mangohud;

use anyhow::{Context, Result, ensure};
use rustix::fs::{CWD, Mode, OFlags, mkfifoat, open};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    time::Instant,
};

const MANGOHUD_DIR: &str = "secondscreen-mangohud";
pub(in crate::ui) const LAUNCH_OPTIONS: &str = r#"MANGOHUD_CONFIG="autostart_log=1,log_interval=0,alpha=0,background_alpha=0,output_folder=$XDG_RUNTIME_DIR/secondscreen-mangohud" mangohud %command%"#;

fn runtime_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is unavailable.")
}

pub(super) fn prepare_mangohud() -> Result<()> {
    let path = runtime_dir()?.join(MANGOHUD_DIR);
    match fs::create_dir(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Could not create {}", path.display())),
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
}

pub(super) struct Feed {
    mango: mangohud::Feed,
    gamescope: Option<Gamescope>,
    gamescope_path: PathBuf,
}

impl Feed {
    pub fn open() -> Result<Self> {
        let dir = runtime_dir()?;
        Ok(Self {
            mango: mangohud::Feed::new(dir.join(MANGOHUD_DIR)),
            gamescope: None,
            gamescope_path: dir.join("secondscreen-fps"),
        })
    }

    pub fn sample(&mut self) -> Result<Option<Reading>> {
        let mango = self.mango.sample();
        // Drain the FIFO even when MangoHud is active so a Gamescope writer can keep running.
        let gamescope = self.sample_gamescope();
        match (mango, gamescope) {
            (Ok(Some(reading)), _) => Ok(Some(reading)),
            (_, Ok(Some(current))) => Ok(Some(Reading {
                source: Source::Gamescope,
                current,
                low_1: None,
                low_01: None,
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
    use std::io::Write;
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
