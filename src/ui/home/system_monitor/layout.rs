use super::GAP;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Placement {
    pub fn with_gap(self) -> Self {
        Self {
            width: self.width + GAP,
            height: self.height + GAP,
            ..self
        }
    }

    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub fn shares_columns(self, other: Self) -> bool {
        self.x < other.right() && other.x < self.right()
    }

    pub fn shares_rows(self, other: Self) -> bool {
        self.y < other.bottom() && other.y < self.bottom()
    }

    fn contains(self, other: Self) -> bool {
        self.x <= other.x
            && self.y <= other.y
            && self.right() >= other.right()
            && self.bottom() >= other.bottom()
    }
}

pub(super) fn place_cards(
    width: f32,
    sizes: impl IntoIterator<Item = (f32, f32)>,
) -> Vec<Placement> {
    let mut free = vec![Placement {
        x: 0.,
        y: 0.,
        width: width + GAP,
        height: f32::INFINITY,
    }];
    let mut placements = Vec::new();
    for (card_width, height) in sizes {
        let reserved_width = card_width + GAP;
        let reserved_height = height + GAP;
        let space = free
            .iter()
            .copied()
            .filter(|space| space.width >= reserved_width && space.height >= reserved_height)
            .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
            .expect("there is space below the placed cards");
        let placement = Placement {
            x: space.x,
            y: space.y,
            width: card_width,
            height,
        };
        let occupied = placement.with_gap();
        placements.push(placement);

        // Keep the full strip on each side, including space beneath a shorter neighbor.
        let mut remaining = Vec::new();
        for space in free {
            if !space.shares_columns(occupied) || !space.shares_rows(occupied) {
                remaining.push(space);
                continue;
            }
            if space.x < occupied.x {
                remaining.push(Placement {
                    width: occupied.x - space.x,
                    ..space
                });
            }
            if occupied.right() < space.right() {
                remaining.push(Placement {
                    x: occupied.right(),
                    width: space.right() - occupied.right(),
                    ..space
                });
            }
            if space.y < occupied.y {
                remaining.push(Placement {
                    height: occupied.y - space.y,
                    ..space
                });
            }
            if occupied.bottom() < space.bottom() {
                remaining.push(Placement {
                    y: occupied.bottom(),
                    height: space.bottom() - occupied.bottom(),
                    ..space
                });
            }
        }
        free = remaining
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, space)| {
                let contained = remaining.iter().enumerate().any(|(other, &candidate)| {
                    other != index
                        && candidate.contains(space)
                        && (candidate != space || other < index)
                });
                (!contained).then_some(space)
            })
            .collect();
    }
    placements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_cards_stack_next_to_a_tall_chart() {
        let positions = place_cards(380., [(232., 150.), (128., 42.), (136., 60.)]);
        assert_eq!(
            positions
                .iter()
                .map(|card| (card.x, card.y))
                .collect::<Vec<_>>(),
            [(0., 0.), (240., 0.), (240., 50.)]
        );
    }

    #[test]
    fn a_later_card_fills_space_below_a_short_card() {
        let positions = place_cards(380., [(128., 42.), (232., 150.), (380., 110.), (128., 42.)]);
        assert_eq!(positions[2].y, 158.);
        assert_eq!((positions[3].x, positions[3].y), (0., 50.));
    }

    #[test]
    fn mixed_sizes_stay_within_the_width_without_overlapping() {
        let widths = [128f32, 136., 168., 232., 368., 472.];
        let heights = [42., 60., 110., 146., 126., 218.];
        for width in [112f32, 204., 266., 379., 448., 697.5] {
            let sizes = (0..32).map(|i| (widths[i % 6].min(width), heights[(i * 5) % 6]));
            let placements = place_cards(width, sizes);
            assert_eq!(placements.len(), 32);
            for (index, &lot) in placements.iter().enumerate() {
                assert!(lot.right() <= width);
                for &other in &placements[..index] {
                    assert!(
                        lot.right() + GAP <= other.x
                            || other.right() + GAP <= lot.x
                            || lot.bottom() + GAP <= other.y
                            || other.bottom() + GAP <= lot.y
                    );
                }
            }
        }
    }
}
