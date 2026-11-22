#[derive(Clone, Default)]
pub struct Metrics {
    pub host: String,
    pub uptime: Option<u64>,
    pub cpu_name: String,
    pub sensors: Vec<Sensor>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub download: f32,
    pub upload: f32,
    pub network_available: bool,
    pub storage: Vec<DiskUsage>,
}

#[derive(Clone)]
pub struct DiskUsage {
    pub id: String,
    pub label: String,
    pub device: String,
    pub used: Option<u64>,
    pub total: u64,
}

#[derive(Clone)]
pub struct Sensor {
    pub id: String,
    pub name: String,
    pub unit: &'static str,
    pub value: Option<f32>,
}

impl Sensor {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        unit: &'static str,
        value: Option<f32>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            unit,
            value: value.filter(|v| v.is_finite()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioKind {
    Output,
    Input,
    Playback,
    Recording,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioId {
    pub kind: AudioKind,
    pub index: u32,
    pub name: String,
}

#[derive(Clone)]
pub struct AudioChannel {
    pub id: AudioId,
    pub label: String,
    pub detail: String,
    pub icon: Option<std::path::PathBuf>,
    pub volume: Option<u16>,
    pub muted: bool,
    pub is_default: bool,
}

#[derive(Default)]
pub struct AudioState {
    pub channels: Vec<AudioChannel>,
    pub issue: Option<String>,
}

pub enum AudioAction {
    Volume(AudioId, u16),
    Mute(AudioId, bool),
    Default(AudioId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone)]
pub enum Artwork {
    File(std::path::PathBuf),
    Remote(String),
}

#[derive(Clone)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<i64>,
    pub artwork: Option<Artwork>,
}

#[derive(Clone)]
pub struct Player {
    pub id: String,
    pub app_id: String,
    pub name: String,
    pub playback: Playback,
    pub track: Option<Track>,
    pub position: Option<i64>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_previous: bool,
    pub can_next: bool,
    pub can_seek: bool,
}

impl Player {
    pub fn can_toggle_playback(&self) -> bool {
        match self.playback {
            Playback::Playing => self.can_pause,
            _ => self.can_play,
        }
    }
}

#[derive(Default)]
pub struct MediaState {
    pub players: Vec<Player>,
    pub issue: Option<String>,
}

pub enum MediaAction {
    Toggle,
    Previous,
    Next,
    Seek {
        track: String,
        position: i64,
        requested_at: std::time::Instant,
    },
}

#[derive(Default)]
pub enum DiscordState {
    #[default]
    Unconfigured,
    Disconnected,
    Authorizing,
    Connected(VoiceState),
    Error(String),
}

#[derive(Clone)]
pub struct VoiceState {
    pub account: String,
    pub muted: bool,
    pub deafened: bool,
    pub channel: Option<VoiceChannel>,
}

#[derive(Clone)]
pub struct VoiceChannel {
    pub id: String,
    pub name: String,
    pub members: Vec<VoiceMember>,
}

#[derive(Clone)]
pub struct VoiceMember {
    pub id: Option<String>,
    pub name: String,
    pub avatar_url: Option<String>,
    pub muted: bool,
    pub deafened: bool,
    pub speaking: bool,
    pub streaming: bool,
}

pub enum DiscordAction {
    Configure(Option<crate::config::DiscordCredentials>),
    Connect,
    ToggleMute,
    ToggleDeafen,
    Leave,
}

pub enum Update {
    Metrics(Metrics),
    Audio(AudioState),
    Media(MediaState),
    MediaSeekFailed {
        requested_at: std::time::Instant,
        error: String,
    },
    Discord(DiscordState),
    ActionError(String),
}
