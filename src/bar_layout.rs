//! Where each bar sits on the terminal row: how the gap and the bar width give way as the bar
//! count grows towards what the width can hold.

/// Which part of its terminal cell a bar fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellHalf {
    /// The whole cell.
    Full,
    /// The left dot column of a cell shared with the next bar (canvas mode only).
    Left,
    /// The right dot column of a cell shared with the previous bar (canvas mode only).
    Right,
}

/// The place of one bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarSlot {
    /// Terminal column, counted from the left edge of the row.
    pub column: usize,
    pub half: CellHalf,
    /// Whether a blank column follows the bar's cell.
    pub gap_after: bool,
}

/// Most bars a row `width` columns wide can hold: one per column, or two per column in canvas
/// mode, where a braille cell has two dot columns.
pub fn capacity(width: usize, canvas: bool) -> usize {
    if canvas { width * 2 } else { width }
}

/// True when the `step`-th of `total` evenly spread picks out of `cells` lands on `index`, so
/// the picks are distributed evenly (Bresenham style).
fn picked(index: usize, cells: usize, total: usize) -> bool {
    (index + 1) * total / cells > index * total / cells
}

/// Places `bar_count` bars on a row `width` columns wide.
///
/// With `gap_allowed` the gap between bars is kept while it fits and then removed from some bars
/// (spread evenly) as bars are added, until none is left. Past that, in `canvas` mode, some bars
/// are squeezed two to a cell. A row that cannot hold all the bars even then is laid out as
/// tight as possible and the treble side runs past `width`, where the renderer clips it.
pub fn layout(bar_count: usize, width: usize, gap_allowed: bool, canvas: bool) -> Vec<BarSlot> {
    if bar_count == 0 {
        return Vec::new();
    }
    let (gaps, pairs) = if canvas && bar_count > width {
        (0, (bar_count - width).min(width))
    } else if gap_allowed {
        (width.saturating_sub(bar_count).min(bar_count), 0)
    } else {
        (0, 0)
    };
    // Cells that hold bars; a pair cell holds two.
    let cells = bar_count - pairs.min(bar_count / 2);
    let pairs = pairs.min(bar_count / 2);
    let mut slots = Vec::with_capacity(bar_count);
    let mut column = 0;
    for cell in 0..cells {
        if picked(cell, cells, pairs) {
            slots.push(BarSlot {
                column,
                half: CellHalf::Left,
                gap_after: false,
            });
            slots.push(BarSlot {
                column,
                half: CellHalf::Right,
                gap_after: false,
            });
            column += 1;
        } else {
            let gap_after = picked(cell, cells, gaps);
            slots.push(BarSlot {
                column,
                half: CellHalf::Full,
                gap_after,
            });
            column += 1 + usize::from(gap_after);
        }
    }
    slots.truncate(bar_count);
    slots
}

/// Columns the laid out row spans.
pub fn row_width(slots: &[BarSlot]) -> usize {
    slots
        .last()
        .map_or(0, |slot| slot.column + 1 + usize::from(slot.gap_after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plenty_of_room_keeps_every_gap() {
        let slots = layout(32, 80, true, false);
        assert!(
            slots
                .iter()
                .all(|s| s.gap_after && s.half == CellHalf::Full)
        );
        assert_eq!(row_width(&slots), 64);
    }

    #[test]
    fn test_gaps_are_removed_one_by_one_as_bars_are_added() {
        for bars in 40..=80 {
            let slots = layout(bars, 80, true, false);
            let gaps = slots.iter().filter(|s| s.gap_after).count();
            assert_eq!(gaps, 80 - bars, "{bars} bars");
            assert_eq!(row_width(&slots), 80, "{bars} bars fill the width");
        }
    }

    #[test]
    fn test_gaps_are_spread_evenly() {
        let slots = layout(60, 80, true, false);
        let positions: Vec<usize> = (0..60).filter(|&i| slots[i].gap_after).collect();
        assert_eq!(positions.len(), 20);
        for pair in positions.windows(2) {
            assert!((2..=4).contains(&(pair[1] - pair[0])), "{positions:?}");
        }
    }

    #[test]
    fn test_gap_off_never_has_a_gap() {
        let slots = layout(32, 200, false, false);
        assert!(slots.iter().all(|s| !s.gap_after));
        assert_eq!(row_width(&slots), 32);
    }

    #[test]
    fn test_blocks_mode_never_shares_a_cell() {
        let slots = layout(120, 80, true, false);
        assert!(slots.iter().all(|s| s.half == CellHalf::Full));
    }

    #[test]
    fn test_canvas_pairs_bars_once_the_gap_is_gone() {
        for bars in 81..=160 {
            let slots = layout(bars, 80, true, true);
            assert_eq!(slots.len(), bars);
            assert!(slots.iter().all(|s| !s.gap_after));
            let lefts = slots.iter().filter(|s| s.half == CellHalf::Left).count();
            let rights = slots.iter().filter(|s| s.half == CellHalf::Right).count();
            assert_eq!((lefts, rights), (bars - 80, bars - 80), "{bars} bars");
            assert_eq!(row_width(&slots), 80, "{bars} bars fill the width");
        }
    }

    #[test]
    fn test_pairs_are_adjacent_and_columns_increase() {
        let slots = layout(100, 80, false, true);
        for pair in slots.windows(2) {
            match (pair[0].half, pair[1].half) {
                (CellHalf::Left, CellHalf::Right) => assert_eq!(pair[0].column, pair[1].column),
                (_, CellHalf::Right) | (CellHalf::Left, _) => panic!("unpaired half"),
                _ => assert_eq!(
                    pair[1].column,
                    pair[0].column + 1 + usize::from(pair[0].gap_after)
                ),
            }
        }
    }

    #[test]
    fn test_capacity_doubles_in_canvas() {
        assert_eq!(capacity(100, false), 100);
        assert_eq!(capacity(100, true), 200);
        assert_eq!(row_width(&layout(200, 100, true, true)), 100);
    }

    #[test]
    fn test_too_many_bars_run_past_the_width() {
        assert_eq!(row_width(&layout(150, 80, true, false)), 150);
        assert_eq!(row_width(&layout(300, 80, true, true)), 220);
    }
}
