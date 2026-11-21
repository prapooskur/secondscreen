use std::cell::RefCell;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
mod native;
use html5ever::{
    tendril::StrTendril,
    tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer},
};
pub(crate) use native::WebTile;
use url::Url;

const HTTPS_ORIGIN: &str = "https://io.github.secondscreen/";
const HTTP_ORIGIN: &str = "http://io.github.secondscreen/";
pub(crate) const DISCORD_HOME: &str = "discord://-/channels/@me";

#[derive(Debug)]
pub(crate) enum Source {
    Page(Url),
    Frame {
        url: Url,
        browser_url: Url,
    },
    Discord(Url),
    Gallery {
        base: Url,
        files: Vec<Url>,
        interval: u16,
    },
}

impl Source {
    pub(crate) fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        ensure!(!input.is_empty(), "Paste a URL or iframe code.");
        ensure!(input.len() <= 32_768, "The viewer source is too long.");
        let frame = input.starts_with('<');
        let url = if frame {
            let src = iframe_source(input).context("The iframe needs a src URL.")?;
            web_url(&src)?
        } else {
            web_url(input)?
        };

        if matches!(url.host_str(), Some("discord.com" | "www.discord.com")) {
            let mut parts = url.path().split('/').skip(1);
            if let (Some("channels"), Some(server), Some(channel)) =
                (parts.next(), parts.next(), parts.next())
            {
                return discord_channel(server, channel)
                    .map(Self::Discord)
                    .context("Paste a Discord voice channel link.");
            }
        }

        if let Some(player) = youtube_player(&url)? {
            return Ok(player);
        }
        if frame {
            return Ok(Self::Frame {
                browser_url: url.clone(),
                url,
            });
        }
        Ok(Self::Page(url))
    }
}

pub(crate) fn discord_channel(server: &str, channel: &str) -> Option<Url> {
    let snowflake = |id: &str| id.parse::<u64>().is_ok() && id.bytes().all(|b| b.is_ascii_digit());
    if (server == "@me" || snowflake(server)) && snowflake(channel) {
        return Url::parse(&format!("discord://-/channels/{server}/{channel}")).ok();
    }
    None
}

pub(crate) fn discord_target(url: &str) -> String {
    if native::has_discord() {
        url.to_owned()
    } else {
        url.replacen("discord://-", "https://discord.com", 1)
    }
}

fn web_url(input: &str) -> Result<Url> {
    let input = input.trim();
    let url = if input.starts_with("//") {
        Url::parse(&format!("https:{input}"))
    } else {
        Url::parse(input)
    }
    .context("Paste a valid HTTP or HTTPS URL.")?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "Use an HTTP or HTTPS URL without embedded credentials."
    );
    Ok(url)
}

#[derive(Default)]
struct FrameSource(RefCell<Option<StrTendril>>);

impl TokenSink for FrameSource {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        if let Token::TagToken(tag) = token
            && tag.kind == TagKind::StartTag
            && &*tag.name == "iframe"
            && let Some(src) = tag
                .attrs
                .into_iter()
                .find(|attr| &*attr.name.local == "src")
        {
            let mut first = self.0.borrow_mut();
            if first.is_none() {
                *first = Some(src.value);
            }
        }
        TokenSinkResult::Continue
    }
}

fn iframe_source(input: &str) -> Option<StrTendril> {
    let tokenizer = Tokenizer::new(FrameSource::default(), Default::default());
    let queue = BufferQueue::default();
    queue.push_back(input.into());
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    tokenizer.sink.0.into_inner()
}

fn youtube_player(url: &Url) -> Result<Option<Source>> {
    let host = url.host_str().unwrap_or_default();
    let youtube = matches!(
        host,
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
    );
    let privacy = matches!(host, "youtube-nocookie.com" | "www.youtube-nocookie.com");
    if !youtube && !privacy && host != "youtu.be" {
        return Ok(None);
    }

    if youtube && url.path() == "/playlist" {
        let list = url
            .query_pairs()
            .find(|(k, _)| k == "list")
            .context("This YouTube playlist link needs a list ID.")?
            .1;
        ensure!(
            !list.is_empty(),
            "This YouTube playlist link needs a list ID."
        );
        let mut player = Url::parse("https://www.youtube.com/embed/videoseries")?;
        player.query_pairs_mut().append_pair("list", &list);
        return Ok(Some(Source::Frame {
            url: player,
            browser_url: url.clone(),
        }));
    }
    let mut path = url.path().split('/').skip(1);
    let first = path.next().unwrap_or_default();
    let id = match first {
        _ if host == "youtu.be" => Some(first.into()),
        "watch" if youtube => url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value),
        "embed" | "shorts" | "live" => path.next().map(Into::into),
        _ => return Ok(None),
    };
    let Some(id) = id else { return Ok(None) };
    if first == "embed" && matches!(id.as_ref(), "videoseries" | "live_stream") {
        return Ok(Some(Source::Frame {
            url: url.clone(),
            browser_url: url.clone(),
        }));
    }
    ensure!(
        id.len() == 11
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "This YouTube link does not contain a video ID."
    );
    let player_host = if privacy {
        "www.youtube-nocookie.com"
    } else {
        "www.youtube.com"
    };
    let mut player = Url::parse(&format!("https://{player_host}/embed/{id}"))?;
    // Only pass playback options that remain meaningful in this viewer.
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "start" | "end" | "list" | "index" | "cc_load_policy" => {
                player.query_pairs_mut().append_pair(&key, &value);
            }
            "t" if let Some(seconds) = youtube_seconds(&value) => {
                player
                    .query_pairs_mut()
                    .append_pair("start", &seconds.to_string());
            }
            _ => {}
        }
    }
    let browser_url = if first == "embed" {
        let mut watch = Url::parse("https://www.youtube.com/watch")?;
        watch.query_pairs_mut().append_pair("v", &id);
        if let Some((_, start)) = player.query_pairs().find(|(key, _)| key == "start") {
            watch.query_pairs_mut().append_pair("t", &start);
        }
        watch
    } else {
        url.clone()
    };
    Ok(Some(Source::Frame {
        url: player,
        browser_url,
    }))
}

fn youtube_seconds(time: &str) -> Option<u32> {
    if let Ok(seconds) = time.parse() {
        return Some(seconds);
    }
    let mut seconds = 0u32;
    let mut digits = time;
    for (suffix, factor) in [('h', 3600), ('m', 60), ('s', 1)] {
        if let Some((number, rest)) = digits.split_once(suffix) {
            seconds = seconds.checked_add(number.parse::<u32>().ok()?.checked_mul(factor)?)?;
            digits = rest;
        }
    }
    digits.is_empty().then_some(seconds)
}

fn frame_document(url: &Url) -> String {
    let src = url
        .as_str()
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; frame-src http: https:">
<title>Second Screen Viewer</title>
<style>html,body,iframe{{width:100%;height:100%;margin:0;border:0;background:#080d13;display:block}}</style>
</head><body><iframe title="Viewer" src="{src}" referrerpolicy="strict-origin-when-cross-origin"
allow="autoplay; encrypted-media; fullscreen; picture-in-picture" allowfullscreen></iframe></body></html>"#
    )
}

pub(crate) fn run(source: Source) -> Result<()> {
    native::standalone(&source)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ViewerMode {
    #[default]
    Web,
    TwitchLive,
    TwitchChat,
    Media,
    Slideshow,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ViewerOptions {
    pub mode: ViewerMode,
    pub source: String,
    pub interval: u16,
}
impl Default for ViewerOptions {
    fn default() -> Self {
        Self {
            mode: ViewerMode::Web,
            source: String::new(),
            interval: 10,
        }
    }
}

impl ViewerOptions {
    pub fn resolve(&self) -> Result<Source> {
        match self.mode {
            ViewerMode::Web => Source::parse(&self.source),
            ViewerMode::TwitchLive | ViewerMode::TwitchChat => {
                let input = self.source.trim();
                let channel = if input.contains("://") {
                    let url = web_url(input)?;
                    ensure!(
                        matches!(
                            url.host_str(),
                            Some("twitch.tv" | "www.twitch.tv" | "m.twitch.tv")
                        ),
                        "Enter a Twitch channel or channel URL."
                    );
                    url.path().trim_matches('/').to_owned()
                } else {
                    input.to_owned()
                };
                ensure!(
                    !channel.is_empty()
                        && channel.len() <= 25
                        && channel
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                    "Enter a Twitch channel name."
                );
                let mut url = if self.mode == ViewerMode::TwitchChat {
                    Url::parse(&format!("https://www.twitch.tv/embed/{channel}/chat"))?
                } else {
                    Url::parse("https://player.twitch.tv/")?
                };
                if self.mode == ViewerMode::TwitchLive {
                    url.query_pairs_mut().append_pair("channel", &channel);
                }
                url.query_pairs_mut()
                    .append_pair("parent", "io.github.secondscreen");
                let browser_url = Url::parse(&format!("https://www.twitch.tv/{channel}"))?;
                Ok(Source::Frame { url, browser_url })
            }
            ViewerMode::Media | ViewerMode::Slideshow => {
                let path = fs::canonicalize(self.source.trim())
                    .context("Choose a readable local file or folder.")?;
                let gallery = self.mode == ViewerMode::Slideshow;
                let files = if gallery {
                    ensure!(path.is_dir(), "Choose a folder for the slideshow.");
                    let mut paths = fs::read_dir(&path)?
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .filter(|p| media_type(p).is_some() && p.is_file())
                        .collect::<Vec<_>>();
                    paths.sort();
                    paths
                } else {
                    ensure!(
                        path.is_file() && media_type(&path).is_some(),
                        "Choose an image or video file."
                    );
                    vec![path.clone()]
                };
                ensure!(
                    !files.is_empty(),
                    "No images or videos were found in this folder."
                );
                let base = Url::from_directory_path(if gallery {
                    path.as_path()
                } else {
                    path.parent().context("Missing file folder")?
                })
                .map_err(|_| anyhow::anyhow!("Invalid media folder"))?;
                let files = files
                    .iter()
                    .map(|p| {
                        Url::from_file_path(p).map_err(|_| anyhow::anyhow!("Invalid media path"))
                    })
                    .collect::<Result<_>>()?;
                Ok(Source::Gallery {
                    base,
                    files,
                    interval: self.interval.clamp(1, 3600),
                })
            }
        }
    }
}

fn media_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "avif" => Some("img"),
        "mp4" | "m4v" | "webm" | "mov" | "ogv" | "avi" | "mpeg" | "mkv" => Some("video"),
        _ => None,
    }
}

impl Source {
    pub fn external_url(&self) -> Option<&Url> {
        match self {
            Self::Page(url) => Some(url),
            Self::Frame { browser_url, .. } => Some(browser_url),
            _ => None,
        }
    }

    fn document(&self) -> Result<(&str, Option<String>)> {
        match self {
            Self::Page(url) => Ok((url.as_str(), None)),
            Self::Frame { url, .. } => Ok((
                if url.scheme() == "http" {
                    HTTP_ORIGIN
                } else {
                    HTTPS_ORIGIN
                },
                Some(frame_document(url)),
            )),
            Self::Discord(_) => bail!("Watch this channel in Discord."),
            Self::Gallery {
                base,
                files,
                interval,
            } => {
                let items = files
                    .iter()
                    .map(|url| {
                        (
                            media_type(Path::new(url.path())).unwrap_or("img"),
                            url.as_str(),
                        )
                    })
                    .collect::<Vec<_>>();
                let json = serde_json::to_string(&items)?.replace('<', r"\u003c");
                let html = include_str!("viewer/gallery.html")
                    .replace("SLIDE_INTERVAL", &interval.to_string())
                    .replace("MEDIA_ITEMS", &json);
                Ok((base.as_str(), Some(html)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_playlists_and_both_twitch_views() {
        let input = "https://www.youtube.com/playlist?list=PL_example";
        let Source::Frame { url, browser_url } = Source::parse(input).unwrap() else {
            panic!("playlist needs an inline player")
        };
        assert_eq!(
            url.as_str(),
            "https://www.youtube.com/embed/videoseries?list=PL_example"
        );
        assert_eq!(browser_url.as_str(), input);
        for (mode, expected) in [
            (
                ViewerMode::TwitchChat,
                "https://www.twitch.tv/embed/example/chat?parent=io.github.secondscreen",
            ),
            (
                ViewerMode::TwitchLive,
                "https://player.twitch.tv/?channel=example&parent=io.github.secondscreen",
            ),
        ] {
            let mut options = ViewerOptions {
                mode,
                source: "https://www.twitch.tv/example/".into(),
                ..Default::default()
            };
            let Source::Frame { url, .. } = options.resolve().unwrap() else {
                panic!("expected Twitch frame")
            };
            assert_eq!(url.as_str(), expected);
            options.source = "https://other.invalid/example".into();
            assert!(options.resolve().is_err());
        }
    }

    #[test]
    fn gallery_uses_readable_supported_files_in_name_order() {
        let dir = std::env::temp_dir().join(format!("secondscreen-slides-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let options = ViewerOptions {
            mode: ViewerMode::Slideshow,
            source: dir.to_string_lossy().into_owned(),
            interval: 3,
        };
        assert!(options.resolve().is_err());
        for name in ["02 clip.webm", "01 image.png", "notes.html"] {
            fs::write(dir.join(name), []).unwrap()
        }
        let source = options.resolve().unwrap();
        let Source::Gallery {
            ref files,
            interval,
            ..
        } = source
        else {
            panic!("expected gallery")
        };
        assert_eq!(interval, 3);
        assert_eq!(files.len(), 2);
        assert!(files[0].path().ends_with("/01%20image.png"));
        let (_, html) = source.document().unwrap();
        assert!(html.unwrap().contains("3 * 1000"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn embeds_only_the_iframe_src_and_preserves_query_parameters() {
        let input = r#"<IFRAME title="src='https://wrong.example/'" onload="alert(1)"
            src='//video.example/player?q=one&amp;size=2' allowfullscreen></IFRAME>"#;
        let Source::Frame { url, .. } = Source::parse(input).unwrap() else {
            panic!("expected an iframe");
        };
        assert_eq!(url.as_str(), "https://video.example/player?q=one&size=2");
        let document = frame_document(&url);
        assert!(document.contains("q=one&amp;size=2"));
        assert!(!document.contains("onload"));
        for input in [
            "<iframe srcdoc='<p>text</p>'></iframe>",
            "<iframe src='file:///etc/passwd'></iframe>",
            "<iframe src='javascript:alert(1)'></iframe>",
            "https://user:password@video.example/",
        ] {
            assert!(Source::parse(input).is_err());
        }
    }

    #[test]
    fn normalizes_youtube_video_and_live_links() {
        for input in [
            "https://www.youtube.com/watch?v=M7lc1UVf-VE&t=1m2s",
            "https://youtu.be/M7lc1UVf-VE?t=62",
            "https://m.youtube.com/shorts/M7lc1UVf-VE?start=62",
            "https://www.youtube.com/live/M7lc1UVf-VE?start=62",
        ] {
            let Source::Frame { url, browser_url } = Source::parse(input).unwrap() else {
                panic!("expected a YouTube player");
            };
            assert_eq!(
                url.as_str(),
                "https://www.youtube.com/embed/M7lc1UVf-VE?start=62"
            );
            assert_eq!(browser_url.as_str(), input);
        }
        let Source::Frame { url, browser_url } = Source::parse(
            r#"<iframe src="https://www.youtube-nocookie.com/embed/M7lc1UVf-VE?start=62"></iframe>"#,
        )
        .unwrap()
        else {
            panic!("expected a privacy-enhanced YouTube player");
        };
        assert_eq!(url.host_str(), Some("www.youtube-nocookie.com"));
        assert_eq!(
            browser_url.as_str(),
            "https://www.youtube.com/watch?v=M7lc1UVf-VE&t=62"
        );
    }

    #[test]
    fn routes_only_valid_discord_channel_links_to_the_client() {
        let Source::Discord(url) =
            Source::parse("https://discord.com/channels/123/456?source=link").unwrap()
        else {
            panic!("expected a Discord link");
        };
        assert_eq!(url.as_str(), "discord://-/channels/123/456");
        assert_eq!(
            discord_channel("@me", "456").unwrap().as_str(),
            "discord://-/channels/@me/456"
        );
        assert!(Source::parse("https://discord.com/channels/123/invalid").is_err());
        assert!(matches!(
            Source::parse("https://discord.com.example/channels/123/456").unwrap(),
            Source::Page(_)
        ));
    }
}
