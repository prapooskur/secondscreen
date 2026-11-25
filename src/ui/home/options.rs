use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MonitorDisplay {
    Bar,
    Chart,
    Gauge,
}

impl MonitorDisplay {
    pub(super) const ALL: [Self; 3] = [Self::Bar, Self::Chart, Self::Gauge];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Bar => "Bar",
            Self::Chart => "Chart",
            Self::Gauge => "Gauge",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MonitorSource {
    Sensor(String),
    Download,
    Upload,
    Drive(String),
    VisibleDrives,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub(super) struct MonitorReading {
    pub source: MonitorSource,
    pub display: MonitorDisplay,
    #[serde(default, skip_serializing_if = "ChartOptions::is_default")]
    pub chart: ChartOptions,
}

impl MonitorReading {
    pub(super) fn new(source: MonitorSource, display: MonitorDisplay) -> Self {
        Self {
            source,
            display,
            chart: ChartOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ChartLayout {
    #[default]
    Separate,
    Overlay,
}

pub(super) const MAX_PLOTS: usize = 4;

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub(super) struct ChartOptions {
    pub layout: ChartLayout,
    pub additional: Vec<MonitorSource>,
}

impl ChartOptions {
    fn is_default(&self) -> bool {
        self.layout == ChartLayout::Separate && self.additional.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(from = "SavedMonitor")]
pub(super) struct MonitorOptions {
    pub show_top_bar: bool,
    pub readings: Vec<MonitorReading>,
}

impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            show_top_bar: true,
            readings: Vec::new(),
        }
    }
}

fn sensor(key: String, display: MonitorDisplay) -> MonitorReading {
    MonitorReading::new(MonitorSource::Sensor(key), display)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SavedMonitor {
    Current {
        show_top_bar: bool,
        readings: Vec<MonitorReading>,
    },
    Legacy(LegacySensors),
}

impl From<SavedMonitor> for MonitorOptions {
    fn from(saved: SavedMonitor) -> Self {
        use MonitorDisplay::{Bar, Chart};
        match saved {
            SavedMonitor::Current {
                show_top_bar,
                readings,
            } => Self {
                show_top_bar,
                readings,
            },
            SavedMonitor::Legacy(old) => Self {
                readings: match old.mode {
                    SensorMode::Chart => vec![sensor(old.chart, Chart)],
                    SensorMode::Pair => {
                        let [first, second] = old.pair;
                        let mut reading = sensor(first, Chart);
                        reading.chart.additional.push(MonitorSource::Sensor(second));
                        vec![reading]
                    }
                    SensorMode::List => old.list.into_iter().map(|key| sensor(key, Bar)).collect(),
                },
                ..Self::default()
            },
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SensorMode {
    Chart,
    #[default]
    Pair,
    List,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LegacySensors {
    mode: SensorMode,
    chart: String,
    pair: [String; 2],
    list: Vec<String>,
}

impl Default for LegacySensors {
    fn default() -> Self {
        Self {
            mode: SensorMode::Pair,
            chart: "cpu".into(),
            pair: ["cpu".into(), "gpu".into()],
            list: vec!["cpu".into(), "gpu".into(), "memory".into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_monitors_are_empty_and_saved_pairs_keep_their_metrics() {
        assert!(MonitorOptions::default().readings.is_empty());
        let options: MonitorOptions =
            serde_json::from_str(r#"{"mode":"pair","pair":["cpu:0","memory"]}"#).unwrap();
        let [chart] = &options.readings[..] else {
            panic!("expected one chart card for a legacy pair");
        };
        assert_eq!(chart.display, MonitorDisplay::Chart);
        assert_eq!(chart.chart.layout, ChartLayout::Separate);
        assert_eq!(chart.source, MonitorSource::Sensor("cpu:0".into()));
        assert_eq!(
            chart.chart.additional,
            [MonitorSource::Sensor("memory".into())]
        );
    }
}
