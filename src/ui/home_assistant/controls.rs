use serde_json::Value;

use super::api::{Action, Device};

// Home Assistant supported_features flags are specific to each entity domain.
const COVER_ACTIONS: &[(u64, &str, &str)] = &[
    (1, "Open", "open_cover"),
    (8, "Stop", "stop_cover"),
    (2, "Close", "close_cover"),
    (16, "Tilt open", "open_cover_tilt"),
    (64, "Stop tilt", "stop_cover_tilt"),
    (32, "Tilt closed", "close_cover_tilt"),
];
const FAN_SPEED: u64 = 1;
const CLIMATE_TARGET_TEMPERATURE: u64 = 1;
const MEDIA_PAUSE: u64 = 1;
const MEDIA_ON: u64 = 128;
const MEDIA_OFF: u64 = 256;
const MEDIA_STOP: u64 = 4096;
const MEDIA_PLAY: u64 = 16384;

impl Device {
    pub(super) fn supported(&self) -> bool {
        match self.domain() {
            "light" | "switch" | "fan" | "input_boolean" | "humidifier" | "climate" | "lock"
            | "button" | "input_button" | "scene" | "script" => true,
            "cover" => COVER_ACTIONS
                .iter()
                .any(|(feature, _, _)| self.supports(*feature)),
            "media_player" => {
                self.supports(MEDIA_PAUSE | MEDIA_PLAY | MEDIA_ON | MEDIA_OFF | MEDIA_STOP)
            }
            _ => false,
        }
    }

    pub(super) fn actions(&self) -> Vec<Action> {
        let on = self.state == "on";
        let power = || {
            if on {
                Action::new("Turn off", "turn_off")
            } else {
                Action::new("Turn on", "turn_on")
            }
        };
        match self.domain() {
            "light" | "switch" | "fan" | "input_boolean" | "humidifier" => vec![power()],
            "button" | "input_button" => vec![Action::new("Press", "press")],
            "scene" => vec![Action::new("Activate", "turn_on")],
            "script" => vec![Action::new("Run", "turn_on")],
            "lock" => match self.state.as_str() {
                "locked" => vec![Action::new("Unlock", "unlock")],
                "locking" | "unlocking" => Vec::new(),
                _ => vec![Action::new("Lock", "lock")],
            },
            "cover" => self.feature_actions(COVER_ACTIONS),
            "climate" => self
                .attributes
                .get("hvac_modes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|mode| *mode != self.state)
                .map(|mode| Action::new(state_label(mode), "set_hvac_mode").with("hvac_mode", mode))
                .collect(),
            "media_player"
                if matches!(self.state.as_str(), "off" | "standby") && self.supports(MEDIA_ON) =>
            {
                vec![Action::new("Turn on", "turn_on")]
            }
            "media_player" => {
                let playback = if self.state == "playing" {
                    (MEDIA_PAUSE, "Pause", "media_pause")
                } else {
                    (MEDIA_PLAY, "Play", "media_play")
                };
                self.feature_actions(&[
                    playback,
                    (MEDIA_STOP, "Stop", "media_stop"),
                    (MEDIA_OFF, "Turn off", "turn_off"),
                ])
            }
            _ => Vec::new(),
        }
    }

    fn feature_actions(&self, actions: &[(u64, &str, &'static str)]) -> Vec<Action> {
        actions
            .iter()
            .filter(|(feature, _, _)| self.supports(*feature))
            .map(|(_, label, service)| Action::new(*label, service))
            .collect()
    }

    pub(super) fn adjustment(&self) -> Option<Adjustment> {
        match self.domain() {
            "light" if self.state == "on" => {
                let value = self.number("brightness")?;
                let percent = (100. * value / 255.).round().max(1.);
                Some(Adjustment {
                    label: format!("Brightness {percent:.0}%"),
                    down: (value > 1.).then(|| {
                        Action::new("−", "turn_on").with("brightness", (value - 26.).max(1.) as u64)
                    }),
                    up: (value < 255.).then(|| {
                        Action::new("+", "turn_on")
                            .with("brightness", (value + 26.).min(255.) as u64)
                    }),
                })
            }
            "fan" if self.state == "on" && self.supports(FAN_SPEED) => {
                let value = self.number("percentage")?;
                let step = self
                    .number("percentage_step")
                    .filter(|step| *step > 0.)
                    .unwrap_or(10.);
                Some(Adjustment {
                    label: format!("Speed {value:.0}%"),
                    down: (value > step).then(|| {
                        Action::new("−", "set_percentage")
                            .with("percentage", (value - step).round() as u64)
                    }),
                    up: (value < 100.).then(|| {
                        Action::new("+", "set_percentage")
                            .with("percentage", (value + step).round().min(100.) as u64)
                    }),
                })
            }
            "climate" if self.supports(CLIMATE_TARGET_TEMPERATURE) => {
                let value = self.number("temperature")?;
                let min = self.number("min_temp")?;
                let max = self.number("max_temp")?;
                let step = self
                    .number("target_temp_step")
                    .filter(|step| *step > 0.)
                    .unwrap_or(1.);
                let target = |label, temperature: f64| {
                    Action::new(label, "set_temperature")
                        .with("temperature", (temperature * 1000.).round() / 1000.)
                };
                Some(Adjustment {
                    label: format!("Target {value}°"),
                    down: (value > min).then(|| target("−", (value - step).max(min))),
                    up: (value < max).then(|| target("+", (value + step).min(max))),
                })
            }
            _ => None,
        }
    }

    pub(super) fn state_label(&self) -> String {
        match (self.domain(), self.state.as_str()) {
            ("button" | "input_button" | "scene", state) if state != "unavailable" => {
                "Ready".into()
            }
            (_, state) => state_label(state),
        }
    }
}

pub(super) struct Adjustment {
    pub label: String,
    pub down: Option<Action>,
    pub up: Option<Action>,
}

fn state_label(state: &str) -> String {
    let mut chars = state.chars();
    chars.next().map_or_else(String::new, |first| {
        format!(
            "{}{}",
            first.to_uppercase(),
            chars.as_str().replace('_', " ")
        )
    })
}
