use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ui) enum SensorMode {
    Chart,
    #[default]
    Pair,
    List,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub(in crate::ui) struct SensorOptions {
    pub mode: SensorMode,
    pub chart: String,
    pub pair: [String; 2],
    pub list: Vec<String>,
}

impl Default for SensorOptions {
    fn default() -> Self {
        Self {
            mode: SensorMode::Pair,
            chart: "cpu".into(),
            pair: ["cpu".into(), "gpu".into()],
            list: vec!["cpu".into(), "gpu".into(), "memory".into()],
        }
    }
}
