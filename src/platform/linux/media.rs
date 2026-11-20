use anyhow::{Context, Result, ensure};
use async_channel::Sender;
use std::{collections::HashMap, time::Duration};
use zbus::{
    blocking::{Connection, Proxy, connection::Builder, fdo::DBusProxy},
    zvariant::{OwnedObjectPath, OwnedValue},
};

use super::Integration;
use crate::model::{Artwork, MediaAction, MediaState, Playback, Player, Track, Update};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

type Properties = HashMap<String, OwnedValue>;

#[derive(Default)]
pub(super) struct Media {
    connection: Option<Connection>,
}

impl Media {
    fn connection(&mut self) -> Result<&Connection> {
        if self.connection.is_none() {
            self.connection = Some(
                Builder::session()?
                    .method_timeout(Duration::from_millis(450))
                    .build()?,
            );
        }
        self.connection
            .as_ref()
            .context("The D-Bus session is unavailable.")
    }

    fn sample(&mut self) -> Result<Vec<Player>> {
        let connection = self.connection()?;
        let bus = DBusProxy::new(connection)?;
        let mut players: Vec<_> = bus
            .list_names()?
            .into_iter()
            .filter(|name| name.as_str().starts_with(PREFIX))
            .filter_map(|name| read_player(connection, name.as_str()).ok())
            .collect();

        players.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(players)
    }

    fn command(&mut self, id: &str, action: MediaAction) -> Result<()> {
        ensure!(id.starts_with(PREFIX), "Invalid MPRIS player.");
        let connection = self.connection()?;
        let player = Proxy::new(connection, id, PATH, PLAYER)?;
        match action {
            MediaAction::Toggle => player.call("PlayPause", &())?,
            MediaAction::Previous => player.call("Previous", &())?,
            MediaAction::Next => player.call("Next", &())?,
            MediaAction::Seek {
                track, position, ..
            } => {
                let track = OwnedObjectPath::try_from(track)
                    .context("This player did not provide a valid track ID.")?;
                player.call("SetPosition", &(track, position))?
            }
        }
        Ok(())
    }
}

fn read_player(connection: &Connection, id: &str) -> Result<Player> {
    let properties = Proxy::new(connection, id, PATH, "org.freedesktop.DBus.Properties")?;
    let values: Properties = properties.call("GetAll", &(PLAYER,))?;
    let name = properties
        .call::<_, _, OwnedValue>("Get", &("org.mpris.MediaPlayer2", "Identity"))
        .ok()
        .and_then(|value| String::try_from(value).ok())
        .unwrap_or_else(|| id.trim_start_matches(PREFIX).to_owned());
    let playback = match string(&values, "PlaybackStatus") {
        Some("Playing") => Playback::Playing,
        Some("Paused") => Playback::Paused,
        _ => Playback::Stopped,
    };
    let track = values
        .get("Metadata")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Properties::try_from(value).ok())
        .and_then(metadata);
    let position = integer(&values, "Position")
        .or_else(|| {
            track.as_ref()?;
            let value: OwnedValue = properties.call("Get", &(PLAYER, "Position")).ok()?;
            i64::try_from(&value).ok()
        })
        .filter(|position| *position >= 0);
    let control = flag(&values, "CanControl");

    Ok(Player {
        id: id.to_owned(),
        name,
        playback,
        track,
        position,
        can_play: control && flag(&values, "CanPlay"),
        can_pause: control && flag(&values, "CanPause"),
        can_previous: control && flag(&values, "CanGoPrevious"),
        can_next: control && flag(&values, "CanGoNext"),
        can_seek: control && flag(&values, "CanSeek"),
    })
}

fn metadata(values: Properties) -> Option<Track> {
    let id = values
        .get("mpris:trackid")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| OwnedObjectPath::try_from(value).ok())
        .map(|path| path.to_string())
        .or_else(|| string(&values, "mpris:trackid").map(str::to_owned))
        .unwrap_or_default();
    let title = string(&values, "xesam:title");
    if (id.is_empty() && title.is_none()) || id == "/org/mpris/MediaPlayer2/TrackList/NoTrack" {
        return None;
    }

    let artist = values
        .get("xesam:artist")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Vec::<String>::try_from(value).ok())
        .filter(|names| !names.is_empty())
        .map(|names| names.join(", "))
        .or_else(|| string(&values, "xesam:artist").map(str::to_owned))
        .unwrap_or_default();
    let artwork = string(&values, "mpris:artUrl")
        .and_then(|value| url::Url::parse(value).ok())
        .and_then(|url| match url.scheme() {
            "file" => url.to_file_path().ok().map(Artwork::File),
            "https" | "http" => Some(Artwork::Remote(url.to_string())),
            _ => None,
        });

    Some(Track {
        id,
        title: title
            .filter(|title| !title.is_empty())
            .unwrap_or("Untitled track")
            .to_owned(),
        artist,
        album: string(&values, "xesam:album")
            .unwrap_or_default()
            .to_owned(),
        duration: integer(&values, "mpris:length").filter(|duration| *duration > 0),
        artwork,
    })
}

fn string<'a>(values: &'a Properties, key: &str) -> Option<&'a str> {
    <&str>::try_from(values.get(key)?).ok()
}

fn integer(values: &Properties, key: &str) -> Option<i64> {
    i64::try_from(values.get(key)?).ok()
}

fn flag(values: &Properties, key: &str) -> bool {
    values
        .get(key)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

impl Integration for Media {
    type Action = (String, MediaAction);

    fn poll(&mut self) -> Update {
        Update::Media(match self.sample() {
            Ok(players) => MediaState {
                players,
                issue: None,
            },
            Err(error) => {
                self.connection = None;
                MediaState {
                    players: Vec::new(),
                    issue: Some(format!("D-Bus media discovery failed. {error}")),
                }
            }
        })
    }

    fn handle(&mut self, (id, action): Self::Action, updates: &Sender<Update>) -> Result<()> {
        let MediaAction::Seek { requested_at, .. } = &action else {
            return self.command(&id, action);
        };
        let requested_at = *requested_at;
        if let Err(error) = self.command(&id, action) {
            let _ = updates.send_blocking(Update::MediaSeekFailed {
                requested_at,
                error: format!("Could not seek. {error:#}"),
            });
        }
        Ok(())
    }
}
