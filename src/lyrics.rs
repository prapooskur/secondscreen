use std::{cmp::Reverse, io::Read, sync::Arc, time::Duration};

use anyhow::{Result, ensure};
use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::model::Track;

const MAX_LRC_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SEARCH_DURATION_DIFFERENCE: f64 = 5.;

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
    #[serde(skip)]
    primary_artist: String,
}

impl LyricsQuery {
    pub fn for_track(track: &Track) -> Option<Self> {
        let title = track.title.trim();
        if title.is_empty() {
            return None;
        }
        let artists: Vec<_> = track
            .artists
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect();
        let primary_artist = primary_artist(artists.first()?).to_owned();
        let album = track.album.trim();
        Some(Self {
            track_name: title.to_owned(),
            artist_name: artists.join(", "),
            album_name: (!album.is_empty()).then(|| album.to_owned()),
            duration: track
                .duration
                .map(|micros| micros.saturating_add(500_000) / 1_000_000)
                .filter(|seconds| (1..=3600).contains(seconds)),
            primary_artist,
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
        self.fetch_from(&client, "https://lrclib.net/api")
    }

    fn fetch_from(&self, client: &Client, endpoint: &str) -> Result<Option<SyncedLyrics>> {
        let exact = request::<LyricsResponse>(client.get(format!("{endpoint}/get")).query(self))?;
        if let Some(lyrics) = exact.and_then(LyricsResponse::into_lyrics) {
            return Ok(Some(lyrics));
        }

        let title = search_title(&self.track_name);
        let candidates =
            request::<Vec<LyricsResponse>>(client.get(format!("{endpoint}/search")).query(&[
                ("track_name", title),
                ("artist_name", self.primary_artist.as_str()),
            ]))?;
        Ok(candidates
            .unwrap_or_default()
            .into_iter()
            .filter_map(|track| {
                let score = self.match_score(&track, title)?;
                Some((score, track.into_lyrics()?))
            })
            .max_by_key(|(score, _)| *score)
            .map(|(_, lyrics)| lyrics))
    }

    fn match_score(
        &self,
        track: &LyricsResponse,
        search_title_name: &str,
    ) -> Option<(bool, bool, bool, Reverse<u64>)> {
        let title = track.track_name.as_deref().or(track.name.as_deref())?;
        let exact_title = name_key(title) == name_key(&self.track_name);
        if !exact_title && name_key(search_title(title)) != name_key(search_title_name) {
            return None;
        }
        let artist = track.artist_name.as_deref()?;
        let expected_artist = artist_key(&self.artist_name);
        let primary = artist_key(&self.primary_artist);
        let candidate = artist_key(artist);
        let exact_artist = candidate == expected_artist;
        if !exact_artist
            && candidate != primary
            && !(expected_artist == primary && artist_key(primary_artist(artist)) == primary)
        {
            return None;
        }

        let same_album = self
            .album_name
            .as_deref()
            .zip(track.album_name.as_deref())
            .is_some_and(|(expected, album)| name_key(expected) == name_key(album));
        let distance = match self.duration {
            Some(seconds) => {
                let duration = track
                    .duration
                    .filter(|duration| duration.is_finite() && *duration > 0.)?;
                let difference = (duration - seconds as f64).abs();
                if difference > MAX_SEARCH_DURATION_DIFFERENCE {
                    return None;
                }
                (difference * 1_000.).round() as u64
            }
            None if same_album || (exact_title && exact_artist) => 0,
            None => return None,
        };
        Some((exact_title, exact_artist, same_album, Reverse(distance)))
    }
}

fn request<T: DeserializeOwned>(request: RequestBuilder) -> Result<Option<T>> {
    let response = request.send()?;
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
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn name_key(name: &str) -> String {
    let key: String = name
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if key.is_empty() {
        name.trim().to_lowercase()
    } else {
        key
    }
}

fn artist_key(artist: &str) -> String {
    let words = artist.split_whitespace();
    let count = words.clone().count();
    let credits: String = words
        .enumerate()
        .filter_map(|(index, word)| {
            let connector = index > 0
                && index + 1 < count
                && is_feature_credit(
                    &word
                        .trim_matches(|ch: char| !ch.is_alphanumeric())
                        .to_ascii_lowercase(),
                );
            (!connector).then_some(word)
        })
        .collect();
    name_key(&credits)
}

fn primary_artist(artist: &str) -> &str {
    let lower = artist.to_ascii_lowercase();
    [
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " featuring ",
        " (feat. ",
        " (ft. ",
        " (featuring ",
    ]
    .into_iter()
    .filter_map(|separator| lower.find(separator))
    .min()
    .map_or(artist, |index| artist[..index].trim_end())
}

fn search_title(mut title: &str) -> &str {
    loop {
        let Some(shorter) = [
            title.rsplit_once(" - "),
            title.rsplit_once(" – "),
            title.rsplit_once(" — "),
            title
                .strip_suffix(')')
                .and_then(|title| title.rsplit_once('(')),
            title
                .strip_suffix(']')
                .and_then(|title| title.rsplit_once('[')),
        ]
        .into_iter()
        .flatten()
        .find_map(|(title, suffix)| {
            let title = title.trim_end();
            (!title.is_empty() && is_title_qualifier(suffix)).then_some(title)
        }) else {
            return title;
        };
        title = shorter;
    }
}

fn is_feature_credit(word: &str) -> bool {
    matches!(word, "feat" | "ft" | "featuring")
}

fn is_title_qualifier(suffix: &str) -> bool {
    let lower = suffix.to_ascii_lowercase();
    let mut words = lower
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty());
    if words.clone().next().is_some_and(is_feature_credit) {
        return true;
    }
    let remastered = |word: &str| matches!(word, "remaster" | "remastered");
    words.clone().any(remastered)
        && words.all(|word| {
            remastered(word)
                || matches!(word, "digital" | "version")
                || word.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LyricsResponse {
    track_name: Option<String>,
    name: Option<String>,
    artist_name: Option<String>,
    album_name: Option<String>,
    duration: Option<f64>,
    synced_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

impl LyricsResponse {
    fn into_lyrics(self) -> Option<SyncedLyrics> {
        if self.instrumental {
            return None;
        }
        SyncedLyrics::parse(self.synced_lyrics.as_deref()?)
    }
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
    fn lrclib_prefers_synced_lyrics_from_the_exact_lookup() {
        let request = "/api/get?track_name=One+%26+Two&artist_name=Test+Artist&album_name=Test+Album&duration=181";
        let (endpoint, server) = lrclib_fixture([(
            request,
            "200 OK",
            r#"{"syncedLyrics":"[00:03.00]Exact lyric"}"#,
        )]);

        let query = lyrics_query("One & Two", &["Test Artist"]);
        let lyrics = query
            .fetch_from(&test_client(), &endpoint)
            .unwrap()
            .unwrap();
        assert_eq!(lyrics.line(0), Some("Exact lyric"));
        server.join().unwrap();
    }

    #[test]
    fn lrclib_search_finds_synced_lyrics_for_a_different_edition() {
        let request = "/api/get?track_name=One+%26+Two+%28feat.+Guest%29+-+2011+Remaster&artist_name=Test+Artist%2C+Guest&album_name=Test+Album&duration=181";
        let search = "/api/search?track_name=One+%26+Two&artist_name=Test+Artist";
        let (endpoint, server) = lrclib_fixture([
            (
                request,
                "200 OK",
                r#"{"syncedLyrics":null,"plainLyrics":"Only plain lyrics"}"#,
            ),
            (
                search,
                "200 OK",
                r#"[
                {"trackName":"One & Two","artistName":"Test Artist & Guest","albumName":"Test Album","duration":181,"syncedLyrics":null},
                {"name":"One & Two","artistName":"Test Artist feat. Guest","albumName":"Original Album","duration":185,"syncedLyrics":"[00:03.00]Correct lyric"},
                {"trackName":"One & Two","artistName":"Test Artist","albumName":"Original Album","duration":181,"syncedLyrics":"[00:03.00]Less complete credit"},
                {"trackName":"One & Two (feat. Guest) - 2011 Remaster","artistName":"Test Artist, Guest","albumName":"Test Album","duration":177,"instrumental":true,"syncedLyrics":"[00:03.00]Instrumental"},
                {"trackName":"One & Two (feat. Guest) - 2011 Remaster","artistName":"Test Artist, Guest","albumName":"Test Album","duration":192,"syncedLyrics":"[00:03.00]Wrong duration"},
                {"trackName":"One & Two - Live","artistName":"Test Artist, Guest","albumName":"Test Album","duration":181,"syncedLyrics":"[00:03.00]Wrong version"},
                {"trackName":"One & Two (feat. Guest) - 2011 Remaster","artistName":"Test Artist feat. Someone Else","albumName":"Test Album","duration":181,"syncedLyrics":"[00:03.00]Wrong collaboration"}
            ]"#,
            ),
        ]);

        let query = lyrics_query(
            "One & Two (feat. Guest) - 2011 Remaster",
            &["Test Artist", "Guest"],
        );
        let lyrics = query
            .fetch_from(&test_client(), &endpoint)
            .unwrap()
            .unwrap();
        assert_eq!(lyrics.line(0), Some("Correct lyric"));
        server.join().unwrap();
    }

    #[test]
    fn lrclib_search_rejects_an_unrelated_result_after_a_missing_exact_match() {
        let request = "/api/get?track_name=One+%26+Two&artist_name=Test+Artist&album_name=Test+Album&duration=181";
        let search = "/api/search?track_name=One+%26+Two&artist_name=Test+Artist";
        let (endpoint, server) = lrclib_fixture([
            (request, "404 Not Found", "{}"),
            (
                search,
                "200 OK",
                r#"[
                {"trackName":"One & Two","artistName":"Another Test Artist","albumName":"Test Album","duration":181,"syncedLyrics":"[00:03.00]Wrong artist"}
            ]"#,
            ),
        ]);

        let query = lyrics_query("One & Two", &["Test Artist"]);
        assert!(
            query
                .fetch_from(&test_client(), &endpoint)
                .unwrap()
                .is_none()
        );
        server.join().unwrap();
    }

    fn lyrics_query(title: &str, artists: &[&str]) -> LyricsQuery {
        LyricsQuery::for_track(&Track {
            id: "/track".into(),
            title: title.into(),
            artists: artists.iter().map(|artist| (*artist).to_owned()).collect(),
            album: "Test Album".into(),
            duration: Some(180_600_000),
            artwork: None,
            lyrics: None,
        })
        .unwrap()
    }

    fn test_client() -> Client {
        Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
    }

    fn lrclib_fixture<const N: usize>(
        responses: [(&'static str, &'static str, &'static str); N],
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/api", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for (path, status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                let mut reader = BufReader::new(&stream);
                reader.read_line(&mut request).unwrap();
                assert_eq!(request.trim_end(), format!("GET {path} HTTP/1.1"));
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
        (endpoint, server)
    }
}
