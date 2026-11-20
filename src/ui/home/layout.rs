use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config;

pub(super) const MAX_COLUMNS: u8 = 4;
pub(super) const MIN_ROWS: u8 = 2;
pub(super) const MAX_ROWS: u8 = 5;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Performance,
    Media,
    Audio,
    Discord,
    Memory,
    Network,
    Storage,
}

impl Kind {
    pub const ALL: [Self; 7] = [
        Self::Performance,
        Self::Media,
        Self::Audio,
        Self::Discord,
        Self::Memory,
        Self::Network,
        Self::Storage,
    ];

    pub fn widget(self) -> Widget {
        Widget {
            kind: self,
            columns: 1,
            rows: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Widget {
    pub kind: Kind,
    pub columns: u8,
    pub rows: u8,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct Layout {
    pub widgets: Vec<Widget>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
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
        layout.widgets.retain_mut(|widget| {
            if seen.contains(&widget.kind) {
                return false;
            }
            seen.push(widget.kind);
            widget.columns = widget.columns.clamp(1, MAX_COLUMNS);
            widget.rows = widget.rows.clamp(MIN_ROWS, MAX_ROWS);
            true
        });
        Ok(layout)
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
}

pub(super) fn place(widgets: impl IntoIterator<Item = Widget>, columns: u8) -> Vec<Placement> {
    let columns = columns.clamp(1, MAX_COLUMNS);
    let width = usize::from(columns);
    let mut used = Vec::<u8>::new();
    let mut cursor = 0;
    widgets
        .into_iter()
        .map(|widget| {
            let span = widget.columns.clamp(1, columns);
            let height = usize::from(widget.rows);
            loop {
                let (x, y) = (cursor % width, cursor / width);
                cursor += 1;
                if x + usize::from(span) > width {
                    continue;
                }
                let mask = ((1u8 << span) - 1) << x;
                let end = y + height;
                if used.len() < end {
                    used.resize(end, 0);
                }
                let rows = &mut used[y..end];
                if rows.iter().any(|row| row & mask != 0) {
                    continue;
                }
                for row in rows {
                    *row |= mask;
                }
                return Placement {
                    widget,
                    x,
                    y,
                    columns: span,
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
    fn mixed_sizes_reflow_without_overlaps_or_losing_saved_widths() {
        let mut layout = Layout::default();
        layout.widgets[0].columns = 3;
        layout.widgets[0].rows = 4;
        layout.widgets[2].columns = 2;
        for columns in 1..=MAX_COLUMNS {
            let placements = place(layout.widgets.iter().copied(), columns);
            let mut cells = HashSet::new();
            for (placement, saved) in placements.iter().zip(&layout.widgets) {
                assert_eq!(placement.widget, *saved);
                assert!(placement.x + usize::from(placement.columns) <= usize::from(columns));
                for y in placement.y..placement.y + usize::from(saved.rows) {
                    for x in placement.x..placement.x + usize::from(placement.columns) {
                        assert!(cells.insert((x, y)));
                    }
                }
            }
        }
        assert_eq!(place(layout.widgets, 4)[0].columns, 3);
    }

    #[test]
    fn constrained_width_survives_vertical_resize_and_dragging_against_the_limit() {
        assert_eq!(shifted_span(4, 0, 1, 2), 4);
        assert_eq!(shifted_span(4, 1, 1, 2), 4);
        assert_eq!(shifted_span(4, -1, 1, 2), 1);
        assert_eq!(shifted_span(1, 1, 1, 4), 2);
    }
}
