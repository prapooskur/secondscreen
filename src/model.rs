use std::time::Instant;

#[derive(Clone, Default)]
pub struct Metrics {
    pub host: String,
    pub uptime: Option<u64>,
    pub cpu_name: String,
    pub sensors: Vec<Sensor>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub network: Option<NetworkRates>,
}

#[derive(Clone, Copy)]
pub struct NetworkRates {
    // Bytes per second.
    pub download: f32,
    pub upload: f32,
}

#[derive(Clone, PartialEq, Eq)]
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

#[derive(Clone, PartialEq, Eq)]
pub struct AudioChannel {
    pub id: AudioId,
    pub label: String,
    pub detail: String,
    pub icon: Option<std::path::PathBuf>,
    pub volume: Option<u16>,
    pub muted: bool,
    pub is_default: bool,
}

#[derive(Default, PartialEq, Eq)]
pub struct AudioState {
    pub channels: Vec<AudioChannel>,
    pub issue: Option<String>,
}

pub const NORMAL_VOLUME_PERCENT: u16 = 100;

#[derive(Clone)]
pub enum AudioAction {
    Volume(AudioId, u16),
    AdjustVolume(AudioId, i16),
    Mute(AudioId, bool),
    ToggleMute(AudioId),
    Default(AudioId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub enum Artwork {
    File(std::path::PathBuf),
    Remote(String),
}

#[derive(Clone, PartialEq, Eq)]
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
    pub position: Option<PlaybackPosition>,
    pub rate: f64,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_previous: bool,
    pub can_next: bool,
    pub can_seek: bool,
}

impl Player {
    pub fn same_details(&self, other: &Self) -> bool {
        let Self {
            id,
            app_id,
            name,
            playback,
            track,
            position: _,
            rate,
            can_play,
            can_pause,
            can_previous,
            can_next,
            can_seek,
        } = self;
        *id == other.id
            && *app_id == other.app_id
            && *name == other.name
            && *playback == other.playback
            && *track == other.track
            && *rate == other.rate
            && *can_play == other.can_play
            && *can_pause == other.can_pause
            && *can_previous == other.can_previous
            && *can_next == other.can_next
            && *can_seek == other.can_seek
    }

    pub fn can_toggle_playback(&self) -> bool {
        match self.playback {
            Playback::Playing => self.can_pause,
            _ => self.can_play,
        }
    }
}

#[derive(Clone, Copy)]
pub struct PlaybackPosition {
    pub reported: i64,
    sampled_at: Instant,
    anchor: (i64, Instant),
}

const MAX_POSITION_LEAD: f64 = 3_000_000.;

impl PlaybackPosition {
    pub fn new(reported: i64, sampled_at: Instant) -> Self {
        Self {
            reported,
            sampled_at,
            anchor: (reported, sampled_at),
        }
    }

    pub fn sampled_at(self) -> Instant {
        self.sampled_at
    }

    pub fn start_from(&mut self, position: i64, at: Instant) {
        self.anchor = (position, at);
    }

    fn projected(self, now: Instant, rate: f64) -> i64 {
        let (position, at) = self.anchor;
        let advance = now.saturating_duration_since(at).as_secs_f64() * rate * 1_000_000.;
        position.saturating_add(advance as i64).max(0)
    }

    pub fn follow(&mut self, previous: Self, rate: f64) {
        let toward = rate.signum() as i64;
        let reported_delta = (self.reported - previous.reported) * toward;
        let projected = previous.projected(self.sampled_at, rate);
        let offset = ((self.reported - projected) * toward) as f64 / rate.abs();
        // A rounded report can lag behind the display clock after a confirmed seek.
        // Resync if the report runs ahead or changes direction.
        if reported_delta == 0
            || (reported_delta > 0 && (-MAX_POSITION_LEAD..1_000_000.).contains(&offset))
        {
            self.anchor = previous.anchor;
        }
    }

    pub fn at(self, now: Instant, rate: f64) -> i64 {
        let projected = self.projected(now, rate);
        // Players can report Playing while position is stalled, for example when buffering.
        let limit = self
            .reported
            .saturating_add((rate * MAX_POSITION_LEAD) as i64);
        if rate > 0. {
            projected.min(limit)
        } else {
            projected.max(limit)
        }
    }
}

#[derive(Default)]
pub struct MediaState {
    pub players: Vec<Player>,
    pub issue: Option<String>,
}

#[derive(Clone)]
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

#[derive(Default, PartialEq, Eq)]
pub enum DiscordState {
    #[default]
    Unconfigured,
    Disconnected,
    Authorizing,
    Connected(VoiceState),
    Error(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct VoiceState {
    pub account: String,
    pub muted: bool,
    pub deafened: bool,
    pub channel: Option<VoiceChannel>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct VoiceChannel {
    pub id: String,
    pub name: String,
    pub members: Vec<VoiceMember>,
}

#[derive(Clone, PartialEq, Eq)]
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
    Storage(Vec<DiskUsage>),
    Audio(AudioState),
    Media(MediaState),
    MediaSeekFailed {
        requested_at: std::time::Instant,
        error: String,
    },
    Discord(DiscordState),
    ActionError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn playback_progress_advances_between_two_second_reports() {
        let start = Instant::now();
        let mut position = PlaybackPosition::new(0, start);
        let mut next_sample = 900;
        for second in 1..=15 {
            while next_sample <= second * 1_000 {
                let report = next_sample / 2_000 * 2_000_000;
                let mut sample = PlaybackPosition::new(
                    report,
                    start + Duration::from_millis(next_sample as u64),
                );
                sample.follow(position, 1.);
                position = sample;
                next_sample += 900;
            }
            assert_eq!(
                position.at(start + Duration::from_secs(second as u64), 1.),
                second * 1_000_000
            );
        }
    }

    #[test]
    fn playback_position_resyncs_after_a_seek_and_limits_stalled_projection() {
        let start = Instant::now();
        let old = PlaybackPosition::new(12_000_000, start);
        let at = start + Duration::from_secs(1);
        let mut seek = PlaybackPosition::new(4_000_000, at);
        seek.follow(old, 2.);
        assert_eq!(seek.at(at + Duration::from_millis(500), 2.), 5_000_000);
        assert_eq!(seek.at(at + Duration::from_secs(12), 2.), 10_000_000);
    }

    #[test]
    fn rounded_position_reports_keep_the_confirmed_seek_clock_advancing() {
        let clicked = Instant::now();
        let mut confirmed =
            PlaybackPosition::new(142_000_000, clicked + Duration::from_millis(700));
        confirmed.start_from(142_500_000, clicked);
        let mut newer = PlaybackPosition::new(144_000_000, clicked + Duration::from_secs(3));
        newer.follow(confirmed, 1.);
        assert_eq!(newer.at(clicked + Duration::from_secs(4), 1.), 146_500_000);
    }
}
