use std::{io::Read, sync::Arc, time::Duration};

use anyhow::{Result, ensure};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::model::Track;

const MAX_LRC_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct SyncedLyrics(Vec<Line>);

#[derive(Debug, PartialEq, Eq)]
struct Line {
    start: i64,
    text: Arc<str>,
}

impl SyncedLyrics {
    pub fn parse(lrc: &str) -> Option<Self> {
        if lrc.len() > MAX_LRC_BYTES {
            return None;
        }
        let mut lines = Vec::new();
        let mut timestamps = Vec::new();
        let mut offset = 0_i64;
        for line in lrc.trim_start_matches('\u{feff}').lines().map(str::trim) {
            if let Some(value) = line
                .strip_prefix("[offset:")
                .and_then(|value| value.strip_suffix(']'))
                && let Ok(millis) = value.trim().parse::<i64>()
            {
                // A positive LRC offset makes the lyrics appear earlier.
                offset = millis.saturating_mul(1_000);
                continue;
            }
            let mut text = line;
            while let Some((time, rest)) =
                text.strip_prefix('[').and_then(|tag| tag.split_once(']'))
                && let Some(start) = timestamp(time)
            {
                timestamps.push(start);
                text = rest.trim_start();
            }
            if !timestamps.is_empty() {
                let text = Arc::<str>::from(text);
                lines.extend(timestamps.drain(..).map(|start| Line {
                    start,
                    text: text.clone(),
                }));
            }
        }
        if !lines.iter().any(|line| !line.text.is_empty()) {
            return None;
        }
        for line in &mut lines {
            line.start = line.start.saturating_sub(offset);
        }
        lines.sort_by_key(|line| line.start);
        lines.dedup_by(|next, previous| {
            if next.start != previous.start {
                return false;
            }
            if previous.text.is_empty() {
                previous.text = next.text.clone();
            }
            true
        });
        Some(Self(lines))
    }

    pub fn passed(&self, position: i64) -> usize {
        self.0.partition_point(|line| line.start <= position)
    }

    pub fn line_count(&self) -> usize {
        self.0.len()
    }

    pub fn line(&self, index: usize) -> Option<&str> {
        let line = self.0.get(index)?;
        (!line.text.is_empty()).then_some(&*line.text)
    }

    pub fn next_boundary(&self, passed: usize, rate: f64) -> Option<i64> {
        if rate > 0. {
            Some(self.0.get(passed)?.start)
        } else {
            self.0.get(passed.checked_sub(1)?)?.start.checked_sub(1)
        }
    }
}

fn timestamp(value: &str) -> Option<i64> {
    let (minutes, seconds) = value.split_once(':')?;
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, "0"));
    let digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(minutes) || !digits(seconds) || !digits(fraction) || fraction.len() > 3 {
        return None;
    }
    let minutes: i64 = minutes.parse().ok()?;
    let seconds: i64 = seconds.parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let millis = fraction.parse::<i64>().ok()? * 10_i64.pow(3 - fraction.len() as u32);
    minutes
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(1_000_000)?
        .checked_add(millis * 1_000)
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct LyricsQuery {
    track_name: String,
    artist_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    album_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<i64>,
}

impl LyricsQuery {
    pub fn for_track(track: &Track) -> Option<Self> {
        let title = track.title.trim();
        let artist = track.artist.trim();
        if title.is_empty() || title == "Untitled track" || artist.is_empty() {
            return None;
        }
        let album = track.album.trim();
        Some(Self {
            track_name: title.to_owned(),
            artist_name: artist.to_owned(),
            album_name: (!album.is_empty()).then(|| album.to_owned()),
            duration: track
                .duration
                .map(|micros| micros.saturating_add(500_000) / 1_000_000)
                .filter(|seconds| (1..=3600).contains(seconds)),
        })
    }

    pub fn fetch(&self) -> Result<Option<SyncedLyrics>> {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent(concat!(
                "SecondScreen/",
                env!("CARGO_PKG_VERSION"),
                " (",
                env!("CARGO_PKG_REPOSITORY"),
                ")"
            ))
            .build()?;
        self.fetch_from(&client, "https://lrclib.net/api/get")
    }

    fn fetch_from(&self, client: &Client, endpoint: &str) -> Result<Option<SyncedLyrics>> {
        let response = client.get(endpoint).query(self).send()?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        response
            .error_for_status()?
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_RESPONSE_BYTES,
            "Lyrics response is too large."
        );
        let track: LyricsResponse = serde_json::from_slice(&bytes)?;
        if track.instrumental {
            return Ok(None);
        }
        Ok(track.synced_lyrics.as_deref().and_then(SyncedLyrics::parse))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LyricsResponse {
    synced_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    use super::*;

    #[test]
    fn lrc_timestamps_follow_the_playhead_and_clear_at_instrumental_gaps() {
        let lyrics = SyncedLyrics::parse(
            "\u{feff}[ti:Example]\r\n[00:04.340]\r\n[00:03.2][0:05.00]Refrain\r\n[offset:+200]",
        )
        .unwrap();
        let current = |position| {
            lyrics
                .passed(position)
                .checked_sub(1)
                .and_then(|i| lyrics.line(i))
        };

        assert_eq!(current(2_999_999), None);
        assert_eq!(current(3_000_000), Some("Refrain"));
        assert_eq!(current(4_140_000), None);
        assert_eq!(current(4_800_000), Some("Refrain"));
        assert_eq!(current(3_500_000), Some("Refrain"));
        assert_eq!(lyrics.next_boundary(1, 1.), Some(4_140_000));
        assert_eq!(lyrics.next_boundary(1, -1.), Some(2_999_999));
    }

    #[test]
    fn lrc_ignores_plain_lyrics_and_bad_timestamps() {
        assert!(
            SyncedLyrics::parse(
                "[ar:Artist]\nPlain lyrics\n[00:03]\n[00:61]Invalid\n[00:04.]Invalid"
            )
            .is_none()
        );
    }

    #[test]
    fn lrclib_lookup_sends_track_metadata_and_reads_only_synced_lyrics() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/api/get", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for (status, body) in [
                ("200 OK", r#"{"syncedLyrics":"[00:03.00]Timed lyric"}"#),
                (
                    "200 OK",
                    r#"{"syncedLyrics":null,"plainLyrics":"Only plain lyrics"}"#,
                ),
                ("404 Not Found", "{}"),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                let mut reader = BufReader::new(&stream);
                reader.read_line(&mut request).unwrap();
                assert_eq!(
                    request.trim_end(),
                    "GET /api/get?track_name=One+%26+Two&artist_name=Test+Artist&album_name=Test+Album&duration=181 HTTP/1.1"
                );
                loop {
                    request.clear();
                    if reader.read_line(&mut request).unwrap() == 0 || request == "\r\n" {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let query = LyricsQuery::for_track(&Track {
            id: "/track".into(),
            title: "One & Two".into(),
            artist: "Test Artist".into(),
            album: "Test Album".into(),
            duration: Some(180_600_000),
            artwork: None,
            lyrics: None,
        })
        .unwrap();
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let lyrics = query.fetch_from(&client, &endpoint).unwrap().unwrap();
        assert_eq!(lyrics.line(0), Some("Timed lyric"));
        assert!(query.fetch_from(&client, &endpoint).unwrap().is_none());
        assert!(query.fetch_from(&client, &endpoint).unwrap().is_none());
        server.join().unwrap();
    }
}
