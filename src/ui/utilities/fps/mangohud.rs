use super::{Reading, Source};
use crate::ui::HISTORY;
use rustix::fs::{Mode, OFlags, open};
use std::{
    collections::VecDeque,
    fs::{self, File, Metadata},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const SECOND_NS: u64 = 1_000_000_000;
const HISTORY_NS: u64 = HISTORY as u64 * SECOND_NS;
const STALE_AFTER: Duration = Duration::from_secs(5);
const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Feed {
    directory: PathBuf,
    log: Option<Log>,
    generation: u64,
}

impl Feed {
    pub fn sample(&mut self, directory: &Path) -> io::Result<Option<Reading>> {
        if self.directory != directory {
            directory.clone_into(&mut self.directory);
            self.log = None;
        }
        let Some((path, metadata)) = latest_log(directory)? else {
            self.log = None;
            return Ok(None);
        };
        if self
            .log
            .as_ref()
            .is_none_or(|log| log.inode != metadata.ino() || log.offset > metadata.len())
        {
            self.log = Some(Log::open(&path, &metadata)?);
            self.generation += 1;
        }
        let log = self.log.as_mut().unwrap();
        if log.read_frames(metadata.len())? {
            self.generation += 1;
        }
        Ok(log.reading(Source::MangoHud(self.generation)))
    }
}

fn latest_log(directory: &Path) -> io::Result<Option<(PathBuf, Metadata)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let cutoff = SystemTime::now() - STALE_AFTER;
    Ok(entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if !name.ends_with(".csv") || name.ends_with("_summary.csv") {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().ok()?;
            (metadata.is_file() && modified >= cutoff).then_some((modified, path, metadata))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, path, metadata)| (path, metadata)))
}

struct Log {
    file: File,
    inode: u64,
    offset: u64,
    frames: VecDeque<Frame>,
}

#[derive(Clone, Copy)]
struct Frame {
    elapsed_ns: u64,
    milliseconds: f32,
}

impl Frame {
    fn parse(line: &[u8]) -> Option<Self> {
        // MangoHud CSV starts with FPS and frametime in ms, and ends with elapsed ns.
        let (_, rest) = std::str::from_utf8(line).ok()?.split_once(',')?;
        let (frametime, rest) = rest.split_once(',')?;
        let milliseconds = frametime.parse::<f32>().ok()?;
        let elapsed_ns = rest.rsplit(',').next()?.trim().parse().ok()?;
        (milliseconds.is_finite() && milliseconds > 0. && (1000. / milliseconds).is_finite())
            .then_some(Self {
                elapsed_ns,
                milliseconds,
            })
    }
}

impl Log {
    fn open(path: &Path, metadata: &Metadata) -> io::Result<Self> {
        let file = File::from(open(
            path,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?);
        let opened = file.metadata()?;
        if !opened.is_file() || opened.ino() != metadata.ino() {
            return Err(io::Error::other("MangoHud log changed while opening it."));
        }
        Ok(Self {
            file,
            inode: opened.ino(),
            offset: 0,
            frames: VecDeque::new(),
        })
    }

    fn read_frames(&mut self, length: u64) -> io::Result<bool> {
        let start = self.offset.max(length.saturating_sub(MAX_READ_BYTES));
        let mut skipped = start != self.offset;
        let mut restarted = skipped;
        if skipped {
            self.frames.clear();
        }
        self.file.seek(SeekFrom::Start(start))?;
        let mut reader = BufReader::new((&mut self.file).take(length - start));
        let mut line = Vec::new();
        self.offset = start;
        while reader.read_until(b'\n', &mut line)? > 0 {
            // Retry an incomplete final row on the next poll.
            if !line.ends_with(b"\n") {
                break;
            }
            self.offset += line.len() as u64;
            if !skipped && let Some(frame) = Frame::parse(&line) {
                if self
                    .frames
                    .back()
                    .is_some_and(|f| f.elapsed_ns > frame.elapsed_ns)
                {
                    self.frames.clear();
                    restarted = true;
                }
                while self
                    .frames
                    .front()
                    .is_some_and(|f| frame.elapsed_ns - f.elapsed_ns >= HISTORY_NS)
                {
                    self.frames.pop_front();
                }
                self.frames.push_back(frame);
            }
            skipped = false;
            line.clear();
        }
        Ok(restarted)
    }

    fn reading(&self, source: Source) -> Option<Reading> {
        let latest = self.frames.back()?;
        let recent_times = self
            .frames
            .iter()
            .rev()
            .take_while(|f| latest.elapsed_ns - f.elapsed_ns < SECOND_NS)
            .map(|f| f.milliseconds);
        let mut times: Vec<_> = self.frames.iter().map(|f| f.milliseconds).collect();
        times.sort_unstable_by(|a, b| b.total_cmp(a));
        let low = |divisor| frame_rate(times[..times.len().div_ceil(divisor)].iter().copied());
        Some(Reading {
            source,
            current: frame_rate(recent_times.clone()),
            low_1: Some(low(100)),
            low_01: Some(low(1000)),
            frame_time_ms: Some(latest.milliseconds),
            peak_frame_time_ms: recent_times.reduce(f32::max),
        })
    }
}

fn frame_rate(times: impl Iterator<Item = f32>) -> f32 {
    let (count, milliseconds) = times.fold((0, 0.), |(count, sum), time| {
        (count + 1, sum + f64::from(time))
    });
    (1000. * f64::from(count) / milliseconds) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_live_logs_and_calculates_lows_from_slowest_frames() {
        let dir = std::env::temp_dir().join(format!("ss-mangohud-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut feed = Feed::default();
        let path = dir.join("game.csv");
        let mut writer = File::create(&path).unwrap();
        writeln!(
            writer,
            "os,cpu,gpu\nLinux,CPU,GPU\nfps,frametime,cpu_load,elapsed"
        )
        .unwrap();
        let mut elapsed = 0;
        for ms in [100].into_iter().chain([20; 9]).chain([10; 990]) {
            elapsed += ms * 1_000_000;
            writeln!(writer, "{},{ms},0,{elapsed}", 1000 / ms).unwrap();
        }
        writeln!(writer, "5,inf,0,{}", elapsed + 1).unwrap();
        write!(writer, "5,200,0,").unwrap();
        let sample = feed.sample(&dir).unwrap().unwrap();
        assert_eq!(sample.current, 100.);
        assert_eq!(sample.frame_time_ms, Some(10.));
        assert_eq!(sample.peak_frame_time_ms, Some(10.));
        assert!((sample.low_1.unwrap() - 35.714).abs() < 0.001);
        assert_eq!(sample.low_01, Some(10.));

        // A split row completes on the following poll and frames outside the window expire.
        writeln!(writer, "{}", elapsed + HISTORY_NS).unwrap();
        let next = feed.sample(&dir).unwrap().unwrap();
        assert_eq!(next.source, sample.source);
        assert_eq!(next.current, 5.);
        assert_eq!(next.frame_time_ms, Some(200.));
        assert_eq!(next.low_01, Some(5.));

        writeln!(writer, "50,20,0,20000000").unwrap();
        let restarted = feed.sample(&dir).unwrap().unwrap();
        assert_ne!(restarted.source, sample.source);
        assert_eq!(restarted.low_01, Some(50.));

        writeln!(writer, "25,40,0,60000000\n50,20,0,80000000").unwrap();
        let sample = feed.sample(&dir).unwrap().unwrap();
        assert_eq!(sample.frame_time_ms, Some(20.));
        assert_eq!(sample.peak_frame_time_ms, Some(40.));

        writer
            .set_modified(SystemTime::now() - STALE_AFTER * 2)
            .unwrap();
        assert!(feed.sample(&dir).unwrap().is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
