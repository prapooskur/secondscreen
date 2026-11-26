use anyhow::{Context, Result, ensure};
use async_channel::Sender;
use futures_lite::{StreamExt, future};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use zbus::{
    MatchRule, Message, MessageStream,
    blocking::{Connection, Proxy, connection::Builder, fdo::DBusProxy},
    message::Type,
    names::OwnedUniqueName,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::Integration;
use crate::lyrics::SyncedLyrics;
use crate::model::{
    Artwork, MediaAction, MediaState, Playback, PlaybackPosition, Player, Track, Update,
};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const POSITION_POLL: Duration = Duration::from_millis(900);
const RECOVERY_POLL: Duration = Duration::from_secs(2);
const RECONCILE_POLL: Duration = Duration::from_secs(10);

type Properties = HashMap<String, OwnedValue>;

#[derive(Default)]
pub(super) struct Media {
    connection: Option<Connection>,
    signals: Option<Signals>,
    players: Vec<Player>,
    next_discovery: Option<Instant>,
    seeking: Option<(String, Instant)>,
}

struct Signals {
    owners: MessageStream,
    players: MessageStream,
    seeked_sender: Option<OwnedUniqueName>,
}

impl Signals {
    async fn new(connection: &zbus::Connection) -> Result<Self> {
        let owners = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg0ns(ROOT)?
            .build();
        let players = MatchRule::builder()
            .msg_type(Type::Signal)
            .path(PATH)?
            .build();
        Ok(Self {
            owners: MessageStream::for_match_rule(owners, connection, None).await?,
            players: MessageStream::for_match_rule(players, connection, None).await?,
            seeked_sender: None,
        })
    }

    async fn changed(&mut self) -> bool {
        loop {
            let message = future::or(self.owners.next(), self.players.next()).await;
            match message {
                Some(Ok(message)) if media_event(&message) => {
                    let header = message.header();
                    if header
                        .member()
                        .is_some_and(|member| member.as_str() == "Seeked")
                    {
                        self.seeked_sender = header.sender().map(|name| name.to_owned().into());
                    }
                    return true;
                }
                Some(Ok(_)) => {}
                _ => return false,
            }
        }
    }
}

fn media_event(message: &Message) -> bool {
    let header = message.header();
    match (
        header.interface().map(|name| name.as_str()),
        header.member().map(|name| name.as_str()),
    ) {
        (Some("org.freedesktop.DBus"), Some("NameOwnerChanged")) => true,
        (Some(PLAYER), Some("Seeked")) => true,
        (Some(PROPERTIES), Some("PropertiesChanged")) => {
            let Ok((interface, changed, invalidated)) =
                message
                    .body()
                    .deserialize::<(String, Properties, Vec<String>)>()
            else {
                return false;
            };
            interface == ROOT
                || (interface == PLAYER
                    && changed
                        .keys()
                        .chain(invalidated.iter())
                        .any(|property| property != "Position"))
        }
        _ => false,
    }
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

    fn discover(&mut self) -> Result<Vec<Player>> {
        let connection = self.connection()?.clone();
        if self.signals.is_none() {
            self.signals = match async_io::block_on(Signals::new(connection.inner())) {
                Ok(signals) => Some(signals),
                Err(error) => {
                    log::debug!("Could not watch MPRIS signals: {error:#}");
                    None
                }
            };
        }
        let seeked_sender = self
            .signals
            .as_mut()
            .and_then(|signals| signals.seeked_sender.take());
        let bus = DBusProxy::new(&connection)?;
        let mut players: Vec<_> = bus
            .list_names()?
            .into_iter()
            .filter(|name| name.as_str().starts_with(PREFIX))
            .filter_map(|name| {
                let mut player = read_player(&connection, name.as_str()).ok()?;
                if let Some(position) = &mut player.position
                    && seeked_sender.as_ref().is_some_and(|sender| {
                        bus.get_name_owner((&name).into())
                            .is_ok_and(|owner| &owner == sender)
                    })
                {
                    position.mark_discontinuity();
                }
                Some(player)
            })
            .collect();

        players.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(players)
    }

    fn discover_update(&mut self) -> Update {
        let issue = match self.discover() {
            Ok(players) => {
                self.players = players;
                let interval = if self.signals.is_some() {
                    RECONCILE_POLL
                } else {
                    RECOVERY_POLL
                };
                self.next_discovery = Some(Instant::now() + interval);
                None
            }
            Err(error) => {
                self.signals = None;
                self.connection = None;
                self.players.clear();
                self.next_discovery = None;
                Some(format!("D-Bus media discovery failed. {error:#}"))
            }
        };
        Update::Media(MediaState {
            players: self.players.clone(),
            issue,
        })
    }

    fn refresh_positions(&mut self) {
        let Some(connection) = &self.connection else {
            return;
        };
        if self
            .seeking
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            self.seeking = None;
        }
        for player in &mut self.players {
            let seeking = self
                .seeking
                .as_ref()
                .is_some_and(|(id, _)| *id == player.id);
            if (player.playback == Playback::Playing || seeking)
                && let Ok(properties) = Proxy::new(connection, player.id.as_str(), PATH, PROPERTIES)
            {
                player.position = read_position(&properties).or_else(|| {
                    let values: Properties = properties.call("GetAll", &(PLAYER,)).ok()?;
                    position(integer(values.get("Position")?)?, Instant::now())
                });
            }
        }
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
    let properties = Proxy::new(connection, id, PATH, PROPERTIES)?;
    let mut values: Properties = properties.call("GetAll", &(PLAYER,))?;
    let sampled_at = Instant::now();
    let mut track = values.remove("Metadata").and_then(metadata);
    // GetAll can omit the duration while a direct Metadata read supplies it.
    if track.as_ref().is_none_or(|track| track.duration.is_none())
        && let Ok(value) = properties.call("Get", &(PLAYER, "Metadata"))
        && let Some(current) = metadata(value)
        && (track.is_none() || current.duration.is_some())
    {
        track = Some(current);
    }
    // Some players return a stale Position in GetAll, including immediately after a seek.
    let position = track
        .as_ref()
        .and_then(|_| read_position(&properties))
        .or_else(|| position(integer(values.get("Position")?)?, sampled_at));
    let root: Properties = properties.call("GetAll", &(ROOT,)).unwrap_or_default();
    let name = string(&root, "Identity")
        .map(str::to_owned)
        .or_else(|| {
            properties
                .call::<_, _, OwnedValue>("Get", &(ROOT, "Identity"))
                .ok()
                .and_then(|value| String::try_from(value).ok())
        })
        .unwrap_or_else(|| id.trim_start_matches(PREFIX).to_owned());
    let app_id = application_id(id, string(&root, "DesktopEntry"));
    let playback = match string(&values, "PlaybackStatus") {
        Some("Playing") => Playback::Playing,
        Some("Paused") => Playback::Paused,
        _ => Playback::Stopped,
    };
    let rate = values
        .get("Rate")
        .and_then(|value| f64::try_from(value).ok())
        .filter(|rate| rate.is_finite() && *rate != 0.)
        .unwrap_or(1.);
    let control = flag(&values, "CanControl");

    Ok(Player {
        id: id.to_owned(),
        app_id,
        name,
        playback,
        track,
        position,
        rate,
        can_play: control && flag(&values, "CanPlay"),
        can_pause: control && flag(&values, "CanPause"),
        can_previous: control && flag(&values, "CanGoPrevious"),
        can_next: control && flag(&values, "CanGoNext"),
        can_seek: control && flag(&values, "CanSeek"),
    })
}

fn position(value: i64, at: Instant) -> Option<PlaybackPosition> {
    (value >= 0).then(|| PlaybackPosition::new(value, at))
}

fn read_position(properties: &Proxy<'_>) -> Option<PlaybackPosition> {
    let value: OwnedValue = properties.call("Get", &(PLAYER, "Position")).ok()?;
    position(integer(&value)?, Instant::now())
}

fn application_id(bus_name: &str, desktop_entry: Option<&str>) -> String {
    if let Some(entry) = desktop_entry
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        return entry.trim_end_matches(".desktop").to_owned();
    }
    let name = bus_name.trim_start_matches(PREFIX);
    name.rsplit_once('.')
        .filter(|(_, instance)| instance.starts_with("instance"))
        .map_or(name, |(application, _)| application)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{OwnedValue, Properties, Value, application_id, metadata};

    #[test]
    fn application_visibility_uses_the_same_key_across_instances() {
        assert_eq!(
            application_id("org.mpris.MediaPlayer2.vlc.instance123", None),
            application_id("org.mpris.MediaPlayer2.vlc.instance456", None)
        );
        assert_eq!(
            application_id(
                "org.mpris.MediaPlayer2.some_instance",
                Some("example.Player.desktop")
            ),
            "example.Player"
        );
        assert_eq!(
            application_id("org.mpris.MediaPlayer2.example.Player", None),
            "example.Player"
        );
    }

    #[test]
    fn track_duration_accepts_signed_and_unsigned_microseconds() {
        let duration = Some(185_000_000);
        for (length, expected) in [
            (Some(OwnedValue::from(185_000_000_i64)), duration),
            (Some(OwnedValue::from(185_000_000_u64)), duration),
            (Some(OwnedValue::from(185_000_000_i32)), duration),
            (Some(OwnedValue::from(185_000_000_u32)), duration),
            (Some(OwnedValue::from(u64::MAX)), None),
            (Some(OwnedValue::from(-1_i64)), None),
            (Some(OwnedValue::from(0_i64)), None),
            (None, None),
        ] {
            let mut values = Properties::from([(
                "xesam:title".into(),
                OwnedValue::try_from(Value::from("Test track")).unwrap(),
            )]);
            if let Some(length) = length {
                values.insert("mpris:length".into(), length);
            }
            assert_eq!(metadata(values.into()).unwrap().duration, expected);
        }
    }
}

fn metadata(value: OwnedValue) -> Option<Track> {
    let values = Properties::try_from(value).ok()?;
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
        duration: values
            .get("mpris:length")
            .and_then(integer)
            .filter(|duration| *duration > 0),
        artwork,
        lyrics: string(&values, "xesam:asText")
            .and_then(SyncedLyrics::parse)
            .map(Arc::new),
    })
}

fn string<'a>(values: &'a Properties, key: &str) -> Option<&'a str> {
    <&str>::try_from(values.get(key)?).ok()
}

fn integer(value: &OwnedValue) -> Option<i64> {
    // MPRIS specifies signed 64-bit microseconds. Some players use another integer width.
    match &**value {
        Value::I64(value) => Some(*value),
        Value::U64(value) => i64::try_from(*value).ok(),
        Value::I32(value) => Some(i64::from(*value)),
        Value::U32(value) => Some(i64::from(*value)),
        _ => None,
    }
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
        if self.next_discovery.is_none_or(|at| Instant::now() >= at) {
            self.discover_update()
        } else {
            self.refresh_positions();
            Update::Media(MediaState {
                players: self.players.clone(),
                issue: None,
            })
        }
    }

    fn poll_interval(&self) -> Duration {
        let Some(at) = self.next_discovery else {
            return RECOVERY_POLL;
        };
        let until_discovery = at.saturating_duration_since(Instant::now());
        if self.seeking.is_some()
            || self
                .players
                .iter()
                .any(|player| player.playback == Playback::Playing)
        {
            until_discovery.min(POSITION_POLL)
        } else {
            until_discovery
        }
    }

    async fn wait_for_event(&mut self) {
        let Some(signals) = &mut self.signals else {
            return future::pending().await;
        };
        if !signals.changed().await {
            self.signals = None;
            self.connection = None;
        }
    }

    fn on_event(&mut self) -> Option<Update> {
        Some(self.discover_update())
    }

    fn handle(&mut self, (id, action): Self::Action, updates: &Sender<Update>) -> Result<()> {
        self.next_discovery = None;
        let MediaAction::Seek { requested_at, .. } = &action else {
            return self.command(&id, action);
        };
        let requested_at = *requested_at;
        match self.command(&id, action) {
            Ok(()) => self.seeking = Some((id, requested_at + Duration::from_secs(3))),
            Err(error) => {
                let _ = updates.send_blocking(Update::MediaSeekFailed {
                    requested_at,
                    error: format!("Could not seek. {error:#}"),
                });
            }
        }
        Ok(())
    }
}
