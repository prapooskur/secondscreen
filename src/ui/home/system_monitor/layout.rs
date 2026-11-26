use super::{Card, GAP, MonitorDisplay};

#[derive(Clone, Copy, Default)]
pub(super) struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Placement {
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

struct Column {
    cards: Vec<usize>,
    width: f32,
    height: f32,
}

#[derive(Default)]
struct Band {
    columns: Vec<Column>,
    width: f32,
    height: f32,
}

impl Band {
    fn push_column(&mut self, index: usize, width: f32, height: f32) {
        self.width += width + if self.columns.is_empty() { 0. } else { GAP };
        self.height = self.height.max(height);
        self.columns.push(Column {
            cards: vec![index],
            width,
            height,
        });
    }

    fn place(
        &mut self,
        index: usize,
        width: f32,
        height: f32,
        available: f32,
        cards: &[Card<'_>],
    ) -> bool {
        if self.width + GAP + width <= available {
            self.push_column(index, width, height);
            return true;
        }
        // A shorter meter column can take another meter beside a taller card.
        if cards[index].display != MonitorDisplay::Chart
            && let Some(column) = self
                .columns
                .iter_mut()
                .filter(|column| {
                    cards[column.cards[0]].display != MonitorDisplay::Chart
                        && column.height + GAP + height <= self.height
                })
                .min_by(|a, b| a.height.total_cmp(&b.height))
        {
            column.cards.push(index);
            column.height += GAP + height;
            return true;
        }
        false
    }

    fn fit_width(&mut self, width: f32, cards: &[Card<'_>]) {
        let extra = (width - self.width) / self.columns.len() as f32;
        self.height = 0.;
        for column in &mut self.columns {
            column.width += extra;
            column.height = gaps(column.cards.len())
                + column
                    .cards
                    .iter()
                    .map(|&index| cards[index].height(column.width))
                    .sum::<f32>();
            self.height = self.height.max(column.height);
        }
    }
}

fn gaps(count: usize) -> f32 {
    count.saturating_sub(1) as f32 * GAP
}

pub(super) fn place_cards(width: f32, height: f32, cards: &[Card<'_>]) -> Vec<Placement> {
    if cards.is_empty() {
        return Vec::new();
    }
    let mut bands: Vec<Band> = Vec::new();
    for (index, card) in cards.iter().enumerate() {
        let card_width = card.width().min(width);
        let card_height = card.height(card_width);
        if !bands
            .iter_mut()
            .any(|band| band.place(index, card_width, card_height, width, cards))
        {
            let mut band = Band::default();
            band.push_column(index, card_width, card_height);
            bands.push(band);
        }
    }
    for band in &mut bands {
        band.fit_width(width, cards);
    }
    let minimum: f32 = bands.iter().map(|band| band.height).sum();
    let stretch = ((height - gaps(bands.len())) / minimum).max(1.);
    let mut positions = vec![Placement::default(); cards.len()];
    let mut y = 0.;
    for band in bands {
        let height = band.height * stretch;
        let mut x = 0.;
        for column in band.columns {
            let extra = (height - column.height) / column.cards.len() as f32;
            let mut top = y;
            for index in column.cards {
                let card_height = cards[index].height(column.width) + extra;
                positions[index] = Placement {
                    x,
                    y: top,
                    width: column.width,
                    height: card_height,
                };
                top += card_height + GAP;
            }
            x += column.width + GAP;
        }
        y += height + GAP;
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::home::system_monitor::{ChartLayout, Reading};

    fn card(display: MonitorDisplay, count: usize) -> Card<'static> {
        Card {
            display,
            layout: ChartLayout::Separate,
            readings: (0..count)
                .map(|index| Reading {
                    name: "Sensor",
                    value: None,
                    unit: if index % 2 == 0 { "%" } else { "B/s" },
                    samples: None,
                    detail: None,
                    tooltip_detail: None,
                    maximum: None,
                })
                .collect(),
        }
    }

    #[test]
    fn a_later_meter_can_fill_a_stack_beside_a_chart() {
        let cards = [
            card(MonitorDisplay::Bar, 1),
            card(MonitorDisplay::Chart, 2),
            card(MonitorDisplay::Chart, 4),
            card(MonitorDisplay::Gauge, 1),
        ];
        let [bar, chart, wide_chart, gauge] = place_cards(380., 340., &cards)[..] else {
            panic!("expected all four cards");
        };
        assert_eq!(chart.y, 0.);
        assert_eq!(bar.y, 0.);
        assert_eq!(bar.x, gauge.x);
        assert_eq!(gauge.y, bar.bottom() + GAP);
        assert_eq!(gauge.bottom(), chart.bottom());
        assert_eq!(wide_chart.y, chart.bottom() + GAP);
        assert_eq!(wide_chart.width, 380.);
        assert_eq!(wide_chart.bottom(), 340.);
    }

    #[test]
    fn the_last_band_of_meters_fills_the_available_width() {
        let cards = std::array::from_fn::<_, 4, _>(|_| card(MonitorDisplay::Bar, 1));
        let positions = place_cards(420., 225., &cards);
        let last = positions[3];
        assert_eq!(last.x, 0.);
        assert_eq!(last.width, 420.);
        assert_eq!(last.bottom(), 225.);
    }

    #[test]
    fn mixed_bands_cover_the_body_and_keep_their_minimum_sizes_when_scrolling() {
        let mut overlay = card(MonitorDisplay::Chart, 3);
        overlay.layout = ChartLayout::Overlay;
        let mut drive = card(MonitorDisplay::Gauge, 1);
        drive.readings[0].detail = Some("Drive unavailable".into());
        let cards = [
            card(MonitorDisplay::Chart, 4),
            drive,
            card(MonitorDisplay::Bar, 1),
            overlay,
            card(MonitorDisplay::Chart, 1),
            card(MonitorDisplay::Gauge, 1),
            card(MonitorDisplay::Bar, 1),
        ];
        let epsilon = 0.001;
        for width in [112., 204., 379., 448., 697.5] {
            for height in [60., 1400.] {
                let positions = place_cards(width, height, &cards);
                let bottom = positions
                    .iter()
                    .map(|card| card.bottom())
                    .fold(0., f32::max);
                let mut area = 0.;
                for (index, &card) in positions.iter().enumerate() {
                    assert!(card.x + card.width <= width + epsilon);
                    assert!(card.height + epsilon >= cards[index].height(card.width));
                    area += (card.width + GAP) * (card.height + GAP);
                    for &other in &positions[..index] {
                        assert!(
                            card.x + card.width + GAP <= other.x + epsilon
                                || other.x + other.width + GAP <= card.x + epsilon
                                || card.bottom() + GAP <= other.y + epsilon
                                || other.bottom() + GAP <= card.y + epsilon
                        );
                    }
                }
                assert!((area - (width + GAP) * (bottom + GAP)).abs() < 0.1);
                if height == 60. {
                    assert!(bottom > height);
                } else {
                    assert!((bottom - height).abs() < epsilon);
                }
            }
        }
    }
}
