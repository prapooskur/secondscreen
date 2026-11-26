use std::collections::VecDeque;

use gpui::{Div, Stateful, Styled, prelude::*};

use crate::ui::{Tooltip, monitor::monitor_graph, theme::*};

use super::{
    CARD_GAP, CARD_INSET, Card, ChartLayout, DETAIL_HEIGHT, GAP, MAX_PLOTS, Reading, card_frame,
    reading_detail,
};

const COLORS: [Color; MAX_PLOTS] = [TEAL, BLUE, AMBER, PINK];
const MIN_PLOT_WIDTH: f32 = 82.;
const SEPARATE_PLOT_HEIGHT: f32 = 59.;
const OVERLAY_PLOT_HEIGHT: f32 = 76.;
const NAME_HEIGHT: f32 = 15.;
const VALUE_HEIGHT: f32 = 17.;
const PLOT_GAP: f32 = 3.;

pub(super) fn separate_width(readings: usize) -> f32 {
    2. * CARD_INSET + readings as f32 * (MIN_PLOT_WIDTH + GAP) - GAP
}

fn plot_columns(width: f32, readings: usize) -> usize {
    let fit = (((width - 2. * CARD_INSET + GAP) / (MIN_PLOT_WIDTH + GAP)).floor() as usize).max(1);
    readings.div_ceil(readings.div_ceil(fit))
}

pub(super) fn chart_height(card: &Card<'_>, width: f32) -> f32 {
    let columns = plot_columns(width, card.readings.len());
    let labels = NAME_HEIGHT + VALUE_HEIGHT;
    let rows = card.readings.len().div_ceil(columns) as f32;
    if card.layout == ChartLayout::Separate || card.readings.len() == 1 {
        let body: f32 = card
            .readings
            .chunks(columns)
            .map(|row| {
                let detail = if row.iter().any(|reading| reading.detail.is_some()) {
                    DETAIL_HEIGHT + PLOT_GAP
                } else {
                    0.
                };
                labels + PLOT_GAP + SEPARATE_PLOT_HEIGHT + detail
            })
            .sum();
        2. * CARD_INSET + body + CARD_GAP * (rows - 1.)
    } else {
        let detail = if has_independent_scales(&card.readings) {
            DETAIL_HEIGHT + CARD_GAP
        } else {
            0.
        };
        2. * CARD_INSET + rows * (labels + CARD_GAP) + OVERLAY_PLOT_HEIGHT + detail
    }
}

pub(super) fn chart_card(
    id: (u32, usize),
    theme: Theme,
    card: &Card<'_>,
    width: f32,
    height: f32,
    single: bool,
) -> Stateful<Div> {
    let columns = plot_columns(width, card.readings.len());
    let extra_height = (height - chart_height(card, width)).max(0.);
    let start = id.1 * MAX_PLOTS;
    let frame = card_frame(id.1, theme, TEAL, single);
    if card.layout == ChartLayout::Separate || card.readings.len() == 1 {
        let plot_height =
            SEPARATE_PLOT_HEIGHT + extra_height / card.readings.len().div_ceil(columns) as f32;
        let mut index = 0;
        return frame.children(card.readings.chunks(columns).map(|readings| {
            let has_details = readings.iter().any(|reading| reading.detail.is_some());
            split()
                .flex_none()
                .gap(r(GAP))
                .children(readings.iter().map(|reading| {
                    let color = COLORS[index];
                    let graph_id = (id.0, start + index);
                    let range = reading.range();
                    index += 1;
                    col()
                        .flex_1()
                        .gap(r(PLOT_GAP))
                        .child(plot_label(theme, index, reading, color, range))
                        .child(chart_area(plot_height).child(monitor_graph(
                            graph_id,
                            theme,
                            vec![(samples(reading, range), color)],
                        )))
                        .children(
                            reading
                                .detail
                                .as_ref()
                                .map(|detail| reading_detail(theme, detail)),
                        )
                        .when(has_details && reading.detail.is_none(), |plot| {
                            plot.child(col().h(r(DETAIL_HEIGHT)).flex_none())
                        })
                }))
        }));
    }

    let (ranges, independent) = overlay_ranges(&card.readings);
    let plots: Vec<_> = card
        .readings
        .iter()
        .zip(ranges.iter().copied())
        .zip(COLORS)
        .collect();
    let mut index = 0;
    frame
        .children(plots.chunks(columns).map(|plots| {
            split().flex_none().gap(r(GAP)).children(plots.iter().map(
                |&((reading, range), color)| {
                    index += 1;
                    plot_label(theme, index, reading, color, range).flex_1()
                },
            ))
        }))
        .child(
            chart_area(OVERLAY_PLOT_HEIGHT + extra_height).child(monitor_graph(
                (id.0, start),
                theme,
                plots
                    .iter()
                    .map(|&((reading, range), color)| (samples(reading, range), color))
                    .collect(),
            )),
        )
        .when(independent, |it| {
            it.child(reading_detail(theme, "Independent scales"))
        })
}

fn chart_area(height: f32) -> Div {
    col().h(r(height)).flex_none()
}

fn plot_label(
    theme: Theme,
    index: usize,
    reading: &Reading<'_>,
    color: Color,
    range: (f32, f32),
) -> Stateful<Div> {
    let tooltip = reading.tooltip(range);
    col()
        .id(("plot-label", index))
        .flex_none()
        .child(
            row().gap(r(4.)).child(theme.dot(color).size(r(5.))).child(
                theme
                    .label(reading.name.to_owned())
                    .min_w_0()
                    .text_size(r(11.))
                    .line_height(r(NAME_HEIGHT))
                    .truncate(),
            ),
        )
        .child(
            mono(reading.value_text())
                .text_color(theme.color(color))
                .text_size(r(13.))
                .line_height(r(VALUE_HEIGHT))
                .truncate(),
        )
        .tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip.clone())).into())
}

fn has_independent_scales(readings: &[Reading<'_>]) -> bool {
    readings.windows(2).any(|pair| pair[0].unit != pair[1].unit)
}

fn overlay_ranges(readings: &[Reading<'_>]) -> (Vec<(f32, f32)>, bool) {
    let mut ranges: Vec<_> = readings.iter().map(Reading::range).collect();
    let independent = has_independent_scales(readings);
    if !independent && let Some(&first) = ranges.first() {
        let shared = ranges.iter().copied().fold(first, |(low, high), range| {
            (low.min(range.0), high.max(range.1))
        });
        ranges.fill(shared);
    }
    (ranges, independent)
}

fn samples(reading: &Reading<'_>, (low, high): (f32, f32)) -> VecDeque<f32> {
    reading
        .samples
        .into_iter()
        .flatten()
        .map(|sample| (sample - low) / (high - low))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_share_a_range_only_for_matching_units() {
        let reading = |unit, value| Reading {
            name: "Sensor",
            value: Some(value),
            unit,
            samples: None,
            detail: None,
            tooltip_detail: None,
            maximum: None,
        };
        assert_eq!(
            overlay_ranges(&[reading("B/s", 512.), reading("B/s", 2048.)]),
            (vec![(0., 2048.); 2], false)
        );
        assert_eq!(
            overlay_ranges(&[reading("%", 42.), reading("B/s", 2048.)]),
            (vec![(0., 100.), (0., 2048.)], true)
        );
    }
}
