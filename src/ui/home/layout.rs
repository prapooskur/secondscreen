use super::options::SensorOptions;
use crate::viewer::ViewerOptions;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config;

pub(super) const MAX_COLUMNS: u8 = 5;
pub(super) const MIN_ROWS: u8 = 2;
pub(super) const MAX_ROWS: u8 = 5;
pub(super) const VISIBLE_ROWS: u8 = 4;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ui) enum Kind {
    Performance,
    Media,
    Audio,
    Discord,
    Memory,
    Network,
    Storage,
    Mixer,
    Viewer,
    WorldClock,
    Calendar,
    Weather,
    Notifications,
    Fps,
}

impl Kind {
    pub(super) const ALL: [Self; 14] = [
        Self::Performance,
        Self::Media,
        Self::Audio,
        Self::Discord,
        Self::Memory,
        Self::Network,
        Self::Storage,
        Self::Mixer,
        Self::Viewer,
        Self::WorldClock,
        Self::Calendar,
        Self::Weather,
        Self::Notifications,
        Self::Fps,
    ];

    pub(super) fn repeatable(self) -> bool {
        matches!(self, Self::Performance | Self::Viewer)
    }

    pub(super) fn widget(self) -> Widget {
        Widget {
            id: self as u32 + 1,
            kind: self,
            columns: 1,
            rows: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Widget {
    #[serde(default)]
    pub id: u32,
    pub kind: Kind,
    pub columns: u8,
    pub rows: u8,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct Layout {
    pub widgets: Vec<Widget>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sensors: BTreeMap<u32, SensorOptions>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub viewers: BTreeMap<u32, ViewerOptions>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sensors: BTreeMap::new(),
            viewers: BTreeMap::new(),
            widgets: [Kind::Performance, Kind::Media, Kind::Audio, Kind::Discord]
                .map(Kind::widget)
                .into(),
        }
    }
}

impl Layout {
    pub fn read(path: &Path) -> Result<Self> {
        let mut layout: Self = config::read_json(path).context("Could not read the Home layout")?;
        let mut seen = Vec::new();
        let mut ids = HashSet::new();
        let reserved: HashSet<_> = layout.widgets.iter().map(|w| w.id).collect();
        let mut fresh = (1..).filter(|id| !reserved.contains(id));
        layout.widgets.retain_mut(|widget| {
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
        layout.sensors.retain(|id, _| {
            layout
                .widgets
                .iter()
                .any(|w| w.id == *id && w.kind == Kind::Performance)
        });
        layout.viewers.retain(|id, _| {
            layout
                .widgets
                .iter()
                .any(|w| w.id == *id && w.kind == Kind::Viewer)
        });
        Ok(layout)
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

    pub fn write(&self, path: &Path) -> Result<()> {
        config::write_json(path, self).context("Could not save the Home layout")
    }
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
    fn upgrades_legacy_tiles_and_keeps_repeated_widgets_independent() {
        let path = std::env::temp_dir().join(format!(
            "secondscreen-layout-instances-{}.json",
            std::process::id()
        ));
        std::fs::write(&path,r#"{"widgets":[{"kind":"viewer","columns":2,"rows":3},{"kind":"performance","columns":1,"rows":4}]}"#).unwrap();
        let mut layout = Layout::read(&path).unwrap();
        layout.add(Kind::Performance);
        layout.add(Kind::Viewer);
        layout.add(Kind::Storage);
        layout.add(Kind::Storage);
        assert_eq!(layout.widgets.len(), 5);
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
        layout.write(&path).unwrap();
        assert_eq!(Layout::read(&path).unwrap(), layout);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mixed_sizes_reflow_without_overlaps_or_losing_saved_widths() {
        let mut layout = Layout::default();
        layout.widgets[0].columns = MAX_COLUMNS;
        layout.widgets[0].rows = MAX_ROWS;
        layout.widgets[2].columns = 2;
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
            rows: 2,
            ..Kind::Performance.widget()
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
