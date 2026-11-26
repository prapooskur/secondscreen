use crate::ui::icons::ProfileIcon;
use crate::viewer::{ViewerMode, ViewerOptions};

use super::{
    layout::{Kind, Layout, MIN_ROWS, VISIBLE_ROWS, Widget},
    options::{ChartLayout, MonitorDisplay, MonitorOptions, MonitorReading, MonitorSource},
    profiles::{Badge, Profile, ProfileDetails},
    quick_controls::QuickAction,
};

#[derive(Clone, Copy)]
pub(in crate::ui) struct ProfilePreset {
    pub name: &'static str,
    pub description: &'static str,
    pub icon: Option<ProfileIcon>,
    tiles: &'static [(Tile, u8, u8)],
    controls: &'static [QuickAction],
}

enum Tile {
    Standard(Kind),
    Monitor(fn() -> MonitorOptions),
    Viewer(ViewerMode),
}

impl ProfilePreset {
    pub const BLANK: Self = Self {
        name: "Blank",
        description: "An empty layout to build from scratch",
        icon: None,
        tiles: &[],
        controls: &[],
    };

    pub const ALL: [Self; 8] = {
        use QuickAction::*;
        use Tile::{Monitor, Standard, Viewer};

        [
            Self::BLANK,
            Self {
                name: "Everyday",
                description: "Weather, local time, and music controls",
                icon: Some(ProfileIcon::Sun),
                tiles: &[
                    (Standard(Kind::Weather), 2, VISIBLE_ROWS),
                    (Standard(Kind::Media), 2, VISIBLE_ROWS),
                    (Standard(Kind::WorldClock), 1, MIN_ROWS),
                    (Standard(Kind::QuickControls), 1, MIN_ROWS),
                ],
                controls: &[
                    PreviousTrack,
                    PlayPause,
                    NextTrack,
                    VolumeDown,
                    SpeakerMute,
                    VolumeUp,
                ],
            },
            Self {
                name: "PC Vitals",
                description: "CPU, GPU, memory, network, and drive usage",
                icon: Some(ProfileIcon::Pulse),
                tiles: &[
                    (Monitor(system_vitals), 3, VISIBLE_ROWS),
                    (Monitor(network_and_drives), 2, VISIBLE_ROWS),
                ],
                controls: &[],
            },
            Self {
                name: "Gaming",
                description: "FPS, frame time, system load, and Discord",
                icon: Some(ProfileIcon::Gaming),
                tiles: &[
                    (Standard(Kind::Fps), 2, VISIBLE_ROWS),
                    (Monitor(game_load), 2, VISIBLE_ROWS),
                    (Standard(Kind::Discord), 1, VISIBLE_ROWS),
                ],
                controls: &[],
            },
            Self {
                name: "Music & Audio",
                description: "Now playing, volume mixer, and playback shortcuts",
                icon: Some(ProfileIcon::Headphones),
                tiles: &[
                    (Standard(Kind::Media), 2, VISIBLE_ROWS),
                    (Standard(Kind::VolumeMixer), 2, VISIBLE_ROWS),
                    (Standard(Kind::QuickControls), 1, VISIBLE_ROWS),
                ],
                controls: &[
                    PreviousTrack,
                    PlayPause,
                    NextTrack,
                    SwitchPlayer,
                    VolumeDown,
                    SpeakerMute,
                    VolumeUp,
                    SwitchOutput,
                ],
            },
            Self {
                name: "Workday",
                description: "Calendar, desktop notifications, and world clock",
                icon: Some(ProfileIcon::Briefcase),
                tiles: &[
                    (Standard(Kind::Calendar), 2, VISIBLE_ROWS),
                    (Standard(Kind::Notifications), 2, VISIBLE_ROWS),
                    (Standard(Kind::WorldClock), 1, VISIBLE_ROWS),
                ],
                controls: &[],
            },
            Self {
                name: "Stream Companion",
                description: "Twitch live and chat with an app mixer",
                icon: Some(ProfileIcon::Film),
                tiles: &[
                    (Viewer(ViewerMode::TwitchLive), 2, VISIBLE_ROWS),
                    (Viewer(ViewerMode::TwitchChat), 2, VISIBLE_ROWS),
                    (Standard(Kind::Mixer), 1, VISIBLE_ROWS),
                ],
                controls: &[],
            },
            Self {
                name: "Control Deck",
                description: "Touch shortcuts, audio devices, and notifications",
                icon: Some(ProfileIcon::Bolt),
                tiles: &[
                    (Standard(Kind::QuickControls), 2, VISIBLE_ROWS),
                    (Standard(Kind::Audio), 1, VISIBLE_ROWS),
                    (Standard(Kind::Notifications), 2, VISIBLE_ROWS),
                ],
                controls: &[
                    SpeakerMute,
                    MicrophoneMute,
                    SwitchOutput,
                    SwitchMicrophone,
                    AudioSettings,
                    PreviousProfile,
                    NextProfile,
                    SwitchTheme,
                    Fullscreen,
                    IntegrationStatus,
                ],
            },
        ]
    };

    pub(super) fn build(self) -> Profile {
        let mut layout = Layout::empty();
        for (id, &(ref tile, columns, rows)) in (1..).zip(self.tiles) {
            let kind = match tile {
                Tile::Standard(kind) => *kind,
                Tile::Monitor(options) => {
                    layout.monitors.insert(id, options());
                    Kind::SystemMonitor
                }
                Tile::Viewer(mode) => {
                    layout.viewers.insert(
                        id,
                        ViewerOptions {
                            mode: *mode,
                            ..Default::default()
                        },
                    );
                    Kind::Viewer
                }
            };
            layout.widgets.push(Widget {
                id,
                kind,
                columns,
                rows,
            });
        }
        if !self.controls.is_empty() {
            layout.quick_controls = Some(self.controls.to_vec());
        }
        Profile {
            layout,
            details: ProfileDetails {
                name: (!self.tiles.is_empty()).then(|| self.name.into()),
                badge: self.icon.map(Badge::Preset),
                ..Default::default()
            },
        }
    }
}

fn system_vitals() -> MonitorOptions {
    use MonitorDisplay::*;
    MonitorOptions {
        show_top_bar: true,
        readings: vec![
            MonitorReading::new(sensor("cpu"), Chart),
            MonitorReading::new(sensor("gpu"), Chart),
            MonitorReading::new(sensor("memory"), Gauge),
        ],
    }
}

fn network_and_drives() -> MonitorOptions {
    use MonitorSource::*;
    MonitorOptions {
        show_top_bar: false,
        readings: vec![
            overlay_chart(Download, vec![Upload]),
            MonitorReading::new(VisibleDrives, MonitorDisplay::Bar),
        ],
    }
}

fn game_load() -> MonitorOptions {
    MonitorOptions {
        show_top_bar: false,
        readings: vec![overlay_chart(
            sensor("cpu"),
            vec![sensor("gpu"), sensor("memory")],
        )],
    }
}

fn sensor(key: &str) -> MonitorSource {
    MonitorSource::Sensor(key.into())
}

fn overlay_chart(source: MonitorSource, additional: Vec<MonitorSource>) -> MonitorReading {
    let mut reading = MonitorReading::new(source, MonitorDisplay::Chart);
    reading.chart.layout = ChartLayout::Overlay;
    reading.chart.additional = additional;
    reading
}
