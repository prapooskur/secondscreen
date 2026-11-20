use anyhow::{Context, Result, bail};
use async_channel::Sender;
use serde::Deserialize;
use std::{collections::BTreeMap, process::Command};

use super::Integration;
use crate::model::{AudioAction, AudioChannel, AudioId, AudioKind, AudioState, Update};

const NORMAL_VOLUME: u64 = 65_536;

#[derive(Default)]
pub(super) struct Mixer {
    channels: Vec<AudioChannel>,
}

#[derive(Deserialize)]
struct PulseInfo {
    default_sink_name: Option<String>,
    default_source_name: Option<String>,
}

#[derive(Deserialize)]
struct PulseChannel {
    index: u32,
    name: Option<String>,
    description: Option<String>,
    channel_map: Option<String>,
    #[serde(default)]
    mute: bool,
    volume: Option<BTreeMap<String, PulseVolume>>,
    sink: Option<u32>,
    source: Option<u32>,
    monitor_source: Option<String>,
    #[serde(default)]
    properties: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct PulseVolume {
    value: u32,
}

fn pactl(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("pactl")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .context("Could not run pactl. Install the PulseAudio client utilities.")?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        bail!("Audio server request failed. {}", details.trim());
    }
    Ok(output.stdout)
}

fn pulse_channels(kind: AudioKind) -> Result<Vec<PulseChannel>> {
    let object = match kind {
        AudioKind::Output => "sinks",
        AudioKind::Input => "sources",
        AudioKind::Playback => "sink-inputs",
        AudioKind::Recording => "source-outputs",
    };
    Ok(serde_json::from_slice(&pactl(&[
        "--format=json",
        "list",
        object,
    ])?)?)
}

impl Mixer {
    fn sample(&self) -> Result<Vec<AudioChannel>> {
        let info: PulseInfo = serde_json::from_slice(&pactl(&["--format=json", "info"])?)?;
        let mut channels = Vec::new();
        for (kind, default) in [
            (AudioKind::Output, info.default_sink_name),
            (AudioKind::Input, info.default_source_name),
            (AudioKind::Playback, None),
            (AudioKind::Recording, None),
        ] {
            channels.extend(
                pulse_channels(kind)?
                    .into_iter()
                    .filter_map(|channel| channel.into_channel(kind, default.as_deref())),
            );
        }
        channels.sort_by(|a, b| {
            b.is_default
                .cmp(&a.is_default)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });
        Ok(channels)
    }

    fn channel(&self, id: &AudioId) -> Result<&AudioChannel> {
        self.channels
            .iter()
            .find(|channel| channel.id == *id)
            .context("That audio channel is no longer available.")
    }

    fn set_volume(&self, id: AudioId, percent: u16) -> Result<()> {
        self.channel(&id)?;
        // The desktop or a hardware volume knob may have changed the levels since the last poll.
        let channel = pulse_channels(id.kind)?
            .into_iter()
            .find(|channel| channel.index == id.index && channel.identity_name() == id.name)
            .context("That audio channel is no longer available.")?;
        let current = channel.channel_levels()?;
        let levels = scale_volume(&current, percent);
        if levels == current {
            return Ok(());
        }

        let command = match id.kind {
            AudioKind::Output => "set-sink-volume",
            AudioKind::Input => "set-source-volume",
            AudioKind::Playback => "set-sink-input-volume",
            AudioKind::Recording => "set-source-output-volume",
        };
        let index = id.index.to_string();
        let levels: Vec<_> = levels.into_iter().map(|level| level.to_string()).collect();
        let mut args = vec![command, index.as_str()];
        args.extend(levels.iter().map(String::as_str));
        pactl(&args)?;
        Ok(())
    }
}

impl PulseChannel {
    fn identity_name(&self) -> &str {
        self.name
            .as_deref()
            .or_else(|| {
                self.properties
                    .get("application.process.id")
                    .map(String::as_str)
            })
            .unwrap_or_default()
    }

    fn channel_levels(&self) -> Result<Vec<u32>> {
        let volume = self
            .volume
            .as_ref()
            .context("This audio stream does not expose volume.")?;
        let map = self
            .channel_map
            .as_deref()
            .context("The audio server did not report the channel order.")?;
        map.split(',')
            .map(|position| {
                volume
                    .get(position.trim())
                    .map(|entry| entry.value)
                    .context("The audio server returned an incomplete channel volume.")
            })
            .collect()
    }

    fn into_channel(self, kind: AudioKind, default: Option<&str>) -> Option<AudioChannel> {
        let is_monitor = self
            .monitor_source
            .as_deref()
            .is_some_and(|name| !name.is_empty() && name != "n/a")
            || self
                .properties
                .get("device.class")
                .is_some_and(|class| class == "monitor")
            || self
                .name
                .as_deref()
                .is_some_and(|name| name.ends_with(".monitor"));
        if kind == AudioKind::Input && is_monitor {
            return None;
        }
        let device = matches!(kind, AudioKind::Output | AudioKind::Input);
        let name = self.identity_name().to_owned();
        let application = self.properties.get("application.name");
        let media = self.properties.get("media.name");
        let label = if device {
            self.description.as_deref().or(Some(name.as_str()))
        } else {
            application.or(media).map(String::as_str)
        }
        .filter(|label| !label.is_empty())
        .unwrap_or("Audio stream")
        .to_owned();
        let detail = if device {
            self.properties.get("device.profile.description")
        } else {
            media
        }
        .filter(|detail| **detail != label)
        .cloned()
        .unwrap_or_default();
        let volume = self
            .volume
            .as_ref()
            .and_then(|volume| volume.values().map(|entry| entry.value).max())
            .map(|value| {
                let percent = (u64::from(value) * 100 + NORMAL_VOLUME / 2) / NORMAL_VOLUME;
                u16::try_from(percent).unwrap_or(u16::MAX)
            });

        Some(AudioChannel {
            is_default: default == Some(name.as_str()),
            id: AudioId {
                kind,
                index: self.index,
                name,
            },
            label,
            detail,
            volume,
            muted: self.mute,
            device: self.sink.or(self.source),
        })
    }
}

fn scale_volume(current: &[u32], percent: u16) -> Vec<u32> {
    let target = (u64::from(percent) * NORMAL_VOLUME + 50) / 100;
    let peak = u64::from(current.iter().copied().max().unwrap_or_default());
    current
        .iter()
        .map(|&level| {
            (u64::from(level) * target + peak / 2)
                .checked_div(peak)
                .unwrap_or(target) as u32
        })
        .collect()
}

impl Integration for Mixer {
    type Action = AudioAction;

    fn poll(&mut self) -> Update {
        let issue = match self.sample() {
            Ok(channels) => {
                self.channels = channels;
                None
            }
            Err(error) => {
                self.channels.clear();
                Some(error.to_string())
            }
        };
        Update::Audio(AudioState {
            channels: self.channels.clone(),
            issue,
        })
    }

    fn handle(&mut self, action: AudioAction, _: &Sender<Update>) -> Result<()> {
        match action {
            AudioAction::Volume(id, volume) => self.set_volume(id, volume)?,
            AudioAction::Mute(id, muted) => {
                self.channel(&id)?;
                let command = match id.kind {
                    AudioKind::Output => "set-sink-mute",
                    AudioKind::Input => "set-source-mute",
                    AudioKind::Playback => "set-sink-input-mute",
                    AudioKind::Recording => "set-source-output-mute",
                };
                pactl(&[
                    command,
                    &id.index.to_string(),
                    if muted { "1" } else { "0" },
                ])?;
            }
            AudioAction::Default(id) => {
                self.channel(&id)?;
                let command = match id.kind {
                    AudioKind::Output => "set-default-sink",
                    AudioKind::Input => "set-default-source",
                    _ => bail!("Only audio devices can be the default."),
                };
                pactl(&[command, &id.name])?;
            }
            AudioAction::Route(stream, device) => {
                self.channel(&stream)?;
                self.channel(&device)?;
                let command = match (stream.kind, device.kind) {
                    (AudioKind::Playback, AudioKind::Output) => "move-sink-input",
                    (AudioKind::Recording, AudioKind::Input) => "move-source-output",
                    _ => bail!("The stream and device have incompatible directions."),
                };
                pactl(&[command, &stream.index.to_string(), &device.name])?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_uneven_volume_in_server_channel_order() {
        let channel: PulseChannel = serde_json::from_str(
            r#"{
                "index": 1,
                "channel_map": "front-left,front-right,front-center",
                "volume": {
                    "front-center": { "value": 16384 },
                    "front-left": { "value": 65536 },
                    "front-right": { "value": 32768 }
                }
            }"#,
        )
        .unwrap();

        let levels = channel.channel_levels().unwrap();
        assert_eq!(scale_volume(&levels, 60), vec![39322, 19661, 9831]);
        assert_eq!(scale_volume(&levels, 0), vec![0, 0, 0]);
        assert_eq!(scale_volume(&[0, 0, 0], 100), vec![65536, 65536, 65536]);
    }
}
