use super::options::SensorOptions;
use super::quick_controls::QuickAction;
use crate::ui::home_assistant::Binding;
use crate::viewer::ViewerOptions;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashSet};

pub(super) const MAX_COLUMNS: u8 = 5;
pub(super) const MIN_ROWS: u8 = 2;
pub(super) const MAX_ROWS: u8 = 5;
pub(super) const VISIBLE_ROWS: u8 = 4;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ui) enum Kind {
    #[serde(alias = "performance")]
    SystemMonitor,
    Media,
    Audio,
    Discord,
    Mixer,
    Viewer,
    WorldClock,
    Calendar,
    Weather,
    Notifications,
    Fps,
    HomeAssistant,
    QuickControls,
}

impl Kind {
    pub(super) const ALL: [Self; 13] = [
        Self::SystemMonitor,
        Self::Media,
        Self::Audio,
        Self::Discord,
        Self::Mixer,
        Self::Viewer,
        Self::WorldClock,
        Self::Calendar,
        Self::Weather,
        Self::Notifications,
        Self::Fps,
        Self::HomeAssistant,
        Self::QuickControls,
    ];

    pub(super) fn repeatable(self) -> bool {
        matches!(
            self,
            Self::SystemMonitor | Self::Viewer | Self::HomeAssistant
        )
    }

    pub(super) fn widget(self) -> Widget {
        Widget {
            id: self as u32 + 1,
            kind: self,
            columns: if self == Self::SystemMonitor { 2 } else { 1 },
            rows: if matches!(self, Self::HomeAssistant | Self::QuickControls) {
                MIN_ROWS
            } else {
                VISIBLE_ROWS
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(super) struct Widget {
    pub id: u32,
    pub kind: Kind,
    pub columns: u8,
    pub rows: u8,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct Layout {
    #[serde(deserialize_with = "read_widgets")]
    pub widgets: Vec<Widget>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sensors: BTreeMap<u32, SensorOptions>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub viewers: BTreeMap<u32, ViewerOptions>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub home_assistant: BTreeMap<u32, Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_controls: Option<Vec<QuickAction>>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            widgets: [Kind::SystemMonitor, Kind::Media, Kind::Audio, Kind::Discord]
                .map(Kind::widget)
                .into(),
            ..Self::empty()
        }
    }
}

impl Layout {
    pub fn empty() -> Self {
        Self {
            widgets: Vec::new(),
            sensors: BTreeMap::new(),
            viewers: BTreeMap::new(),
            home_assistant: BTreeMap::new(),
            quick_controls: None,
        }
    }

    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        let mut ids = HashSet::new();
        let reserved: HashSet<_> = self.widgets.iter().map(|w| w.id).collect();
        let mut fresh = (1..).filter(|id| !reserved.contains(id));
        self.widgets.retain_mut(|widget| {
            if !widget.kind.repeatable() && seen.contains(&widget.kind) {
                return false;
            }
            seen.push(widget.kind);
            if widget.id == 0 || !ids.insert(widget.id) {
                widget.id = fresh.next().expect("widget ID space exhausted");
                ids.insert(widget.id);
            }
            widget.columns = widget.columns.clamp(1, MAX_COLUMNS);
            widget.rows = widget.rows.clamp(MIN_ROWS, MAX_ROWS);
            true
        });
        self.sensors.retain(|id, _| {
            self.widgets
                .iter()
                .any(|w| w.id == *id && w.kind == Kind::SystemMonitor)
        });
        self.viewers.retain(|id, _| {
            self.widgets
                .iter()
                .any(|w| w.id == *id && w.kind == Kind::Viewer)
        });
        self.home_assistant.retain(|id, _| {
            self.widgets
                .iter()
                .any(|w| w.id == *id && w.kind == Kind::HomeAssistant)
        });
        if !seen.contains(&Kind::QuickControls) {
            self.quick_controls = None;
        } else if let Some(actions) = &mut self.quick_controls {
            let mut unique = HashSet::new();
            actions.retain(|action| unique.insert(*action));
        }
    }

    pub fn quick_actions(&self) -> &[QuickAction] {
        self.quick_controls
            .as_deref()
            .unwrap_or(QuickAction::DEFAULT)
    }

    pub fn add(&mut self, kind: Kind) {
        if kind.repeatable() || !self.widgets.iter().any(|w| w.kind == kind) {
            let id = (1..)
                .find(|id| self.widgets.iter().all(|w| w.id != *id))
                .expect("widget ID space exhausted");
            self.widgets.push(Widget {
                id,
                ..kind.widget()
            });
        }
    }
}

fn read_widgets<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Widget>, D::Error> {
    #[derive(Deserialize)]
    enum SavedKind {
        #[serde(rename = "memory", alias = "storage", alias = "network")]
        LegacyMonitor,
        #[serde(untagged)]
        Current(Kind),
    }

    #[derive(Deserialize)]
    struct SavedWidget {
        #[serde(default)]
        id: u32,
        kind: SavedKind,
        columns: u8,
        rows: u8,
    }

    let saved = Vec::<SavedWidget>::deserialize(deserializer)?;
    // Performance tiles own sensor settings. A layout with only the other metrics needs one monitor.
    let mut has_monitor = saved
        .iter()
        .any(|widget| matches!(widget.kind, SavedKind::Current(Kind::SystemMonitor)));
    Ok(saved
        .into_iter()
        .filter_map(|widget| {
            let kind = match widget.kind {
                SavedKind::Current(kind) => kind,
                SavedKind::LegacyMonitor if has_monitor => return None,
                SavedKind::LegacyMonitor => {
                    has_monitor = true;
                    Kind::SystemMonitor
                }
            };
            Some(Widget {
                id: widget.id,
                kind,
                columns: widget.columns,
                rows: widget.rows,
            })
        })
        .collect())
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Placement {
    pub widget: Widget,
    pub x: usize,
    pub y: usize,
    pub columns: u8,
    pub rows: u8,
}

pub(super) fn place(widgets: impl IntoIterator<Item = Widget>, columns: u8) -> Vec<Placement> {
    let columns = columns.clamp(1, MAX_COLUMNS);
    let height = usize::from(VISIBLE_ROWS);
    let mut used = Vec::<u8>::new();
    let mut cursor = 0;
    widgets
        .into_iter()
        .map(|widget| {
            let span = widget.columns.clamp(1, columns);
            let rows = widget.rows.clamp(MIN_ROWS, VISIBLE_ROWS);
            loop {
                let (x, y) = (cursor / height, cursor % height);
                cursor += 1;
                if y + usize::from(rows) > height {
                    continue;
                }
                let mask = ((1u8 << rows) - 1) << y;
                let end = x + usize::from(span);
                if used.len() < end {
                    used.resize(end, 0);
                }
                let occupied = &mut used[x..end];
                if occupied.iter().any(|column| column & mask != 0) {
                    continue;
                }
                for column in occupied {
                    *column |= mask;
                }
                return Placement {
                    widget,
                    x,
                    y,
                    columns: span,
                    rows,
                };
            }
        })
        .collect()
}

pub(super) fn shifted_span(saved: u8, delta: i32, min: u8, max: u8) -> u8 {
    if delta == 0 {
        return saved;
    }
    let visible = i32::from(saved.min(max));
    let next = (visible + delta).clamp(i32::from(min), i32::from(max));
    if next == visible { saved } else { next as u8 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn quick_controls_preserve_defaults_and_explicit_order_in_saved_layouts() {
        let mut layout: Layout =
            serde_json::from_str(r#"{"widgets":[{"kind":"quick_controls","columns":1,"rows":2}]}"#)
                .unwrap();
        layout.normalize();
        assert_eq!(layout.quick_actions(), QuickAction::DEFAULT);

        let order = [QuickAction::SwitchTheme, QuickAction::PlayPause];
        layout.quick_controls = Some(vec![order[0], order[1], order[0]]);
        let mut saved: Layout =
            serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
        saved.normalize();
        assert_eq!(saved.quick_actions(), order);

        layout.quick_controls = Some(Vec::new());
        let saved: Layout = serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
        assert!(saved.quick_actions().is_empty());
    }

    #[test]
    fn upgrades_legacy_tiles_and_keeps_repeated_widgets_independent() {
        let path = std::env::temp_dir().join(format!(
            "secondscreen-layout-instances-{}.json",
            std::process::id()
        ));
        std::fs::write(&path,r#"{"widgets":[{"kind":"viewer","columns":2,"rows":3},{"kind":"performance","columns":1,"rows":4}]}"#).unwrap();
        let mut layout: Layout = crate::config::read_json(&path).unwrap();
        layout.normalize();
        layout.add(Kind::SystemMonitor);
        layout.add(Kind::Viewer);
        layout.add(Kind::Calendar);
        layout.add(Kind::Calendar);
        assert_eq!(layout.widgets.len(), 5);
        assert_eq!(layout.widgets[0].rows, 3);
        assert!(layout.widgets[2..].iter().all(|w| w.rows == VISIBLE_ROWS));
        assert_eq!(
            layout
                .widgets
                .iter()
                .map(|w| w.id)
                .collect::<HashSet<_>>()
                .len(),
            5
        );
        let first = layout.widgets[0].id;
        let second = layout.widgets[3].id;
        for (id, source) in [
            (first, "https://example.com"),
            (second, "https://example.org"),
        ] {
            layout.viewers.insert(
                id,
                ViewerOptions {
                    source: source.into(),
                    ..Default::default()
                },
            );
        }
        layout
            .sensors
            .entry(layout.widgets[2].id)
            .or_default()
            .chart = "memory".into();
        layout.widgets.swap(0, 3);
        crate::config::write_json(&path, &layout).unwrap();
        let mut saved: Layout = crate::config::read_json(&path).unwrap();
        saved.normalize();
        assert_eq!(saved, layout);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn combines_legacy_monitor_tiles_and_preserves_sensor_instances() {
        let mut layout: Layout = serde_json::from_str(
            r#"{
                "widgets": [
                    {"kind":"network","id":1,"columns":1,"rows":3},
                    {"kind":"performance","id":6,"columns":2,"rows":4},
                    {"kind":"storage","id":2,"columns":1,"rows":4},
                    {"kind":"media","id":3,"columns":1,"rows":4},
                    {"kind":"memory","id":4,"columns":1,"rows":2},
                    {"kind":"performance","id":7,"columns":1,"rows":3}
                ],
                "sensors": {
                    "6":{"mode":"chart","chart":"cpu:0"},
                    "7":{"mode":"list","list":["memory","gpu"]}
                }
            }"#,
        )
        .unwrap();
        layout.normalize();
        let tiles: Vec<_> = layout.widgets.iter().map(|w| (w.kind, w.id)).collect();
        assert_eq!(
            tiles,
            [
                (Kind::SystemMonitor, 6),
                (Kind::Media, 3),
                (Kind::SystemMonitor, 7)
            ]
        );
        assert_eq!(layout.sensors[&6].chart, "cpu:0");
        assert_eq!(layout.sensors[&7].list, ["memory", "gpu"]);
        let saved = serde_json::to_string(&layout).unwrap();
        assert_eq!(saved.matches("system_monitor").count(), 2);
        assert_eq!(serde_json::from_str::<Layout>(&saved).unwrap(), layout);
    }

    #[test]
    fn legacy_memory_storage_and_network_can_upgrade_without_performance() {
        for kind in ["memory", "storage", "network"] {
            let saved =
                format!(r#"{{"widgets":[{{"kind":"{kind}","id":8,"columns":1,"rows":2}}]}}"#);
            let mut layout: Layout = serde_json::from_str(&saved).unwrap();
            layout.normalize();
            assert_eq!(
                layout.widgets,
                [Widget {
                    id: 8,
                    kind: Kind::SystemMonitor,
                    columns: 1,
                    rows: 2,
                }]
            );
        }
    }

    #[test]
    fn mixed_sizes_reflow_without_overlaps_or_losing_saved_widths() {
        let mut layout = Layout::default();
        layout.widgets[0].columns = MAX_COLUMNS;
        layout.widgets[0].rows = MAX_ROWS;
        layout.widgets[2].columns = 2;
        layout.widgets[2].rows = 3;
        for columns in 1..=MAX_COLUMNS {
            let placements = place(layout.widgets.iter().copied(), columns);
            let mut cells = HashSet::new();
            for (placement, saved) in placements.iter().zip(&layout.widgets) {
                assert_eq!(placement.widget, *saved);
                assert!(placement.columns <= columns);
                assert!(placement.y + usize::from(placement.rows) <= usize::from(VISIBLE_ROWS));
                for y in placement.y..placement.y + usize::from(placement.rows) {
                    for x in placement.x..placement.x + usize::from(placement.columns) {
                        assert!(cells.insert((x, y)));
                    }
                }
            }
        }
        assert_eq!(place(layout.widgets, MAX_COLUMNS)[0].columns, MAX_COLUMNS);
    }

    #[test]
    fn overflow_extends_to_the_right_and_keeps_stacked_widgets_together() {
        let tile = Widget {
            columns: 1,
            rows: 2,
            ..Kind::SystemMonitor.widget()
        };
        let placements = place([tile; 14], MAX_COLUMNS);
        for (index, placement) in placements.iter().enumerate() {
            assert_eq!((placement.x, placement.y), (index / 2, index % 2 * 2));
        }
    }

    #[test]
    fn constrained_width_survives_vertical_resize_and_dragging_against_the_limit() {
        assert_eq!(shifted_span(4, 0, 1, 2), 4);
        assert_eq!(shifted_span(4, 1, 1, 2), 4);
        assert_eq!(shifted_span(4, -1, 1, 2), 1);
        assert_eq!(shifted_span(1, 1, 1, 4), 2);
    }
}
