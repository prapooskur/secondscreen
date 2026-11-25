use std::collections::VecDeque;

use gpui::{Div, Stateful, Styled, prelude::*};

use crate::ui::{Tooltip, monitor::monitor_graph, theme::*};

use super::{Card, ChartLayout, GAP, MAX_PLOTS, Reading, card_frame, reading_detail};

const COLORS: [Color; MAX_PLOTS] = [TEAL, BLUE, AMBER, PINK];
const MIN_PLOT_WIDTH: f32 = 82.;

pub(super) fn chart_card(
    id: (u32, usize),
    theme: Theme,
    card: &Card<'_>,
    width: f32,
    extra_height: f32,
) -> Stateful<Div> {
    let columns = (((width - 16. + GAP) / (MIN_PLOT_WIDTH + GAP)).floor() as usize).max(1);
    let start = id.1 * MAX_PLOTS;
    let frame = card_frame(id.1, theme, TEAL).w(r(width));
    if card.layout == ChartLayout::Separate || card.readings.len() == 1 {
        let plot_height = 59. + extra_height / card.readings.len().div_ceil(columns) as f32;
        let mut index = 0;
        return frame.children(card.readings.chunks(columns).map(|readings| {
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
                        .gap(r(3.))
                        .child(plot_label(theme, index, reading, color, range))
                        .child(col().h(r(plot_height)).flex_none().child(monitor_graph(
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
            col()
                .h(r(76. + extra_height))
                .flex_none()
                .child(monitor_graph(
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
                    .text_size(r(11.))
                    .line_height(r(15.))
                    .truncate(),
            ),
        )
        .child(
            mono(reading.value_text())
                .text_color(theme.color(color))
                .text_size(r(13.))
                .line_height(r(17.))
                .truncate(),
        )
        .tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip.clone())).into())
}

fn overlay_ranges(readings: &[Reading<'_>]) -> (Vec<(f32, f32)>, bool) {
    let mut ranges: Vec<_> = readings.iter().map(Reading::range).collect();
    let independent = readings.windows(2).any(|pair| pair[0].unit != pair[1].unit);
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
