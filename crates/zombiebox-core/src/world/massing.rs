//! Small deterministic helpers for composing authored building footprints.
//!
//! This is intentionally a mask rather than a lot solver: Phase 3.3 uses it
//! to describe the sparse union of a house, porch, and garage wing.

use std::collections::BTreeSet;

use super::blueprint::{GridRect, LocalCell};

/// A stable two-dimensional set of occupied local tiles.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FootprintMask {
    cells: BTreeSet<(i32, i32)>,
}

impl FootprintMask {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an inclusive local rectangle, matching `GridRect` semantics.
    pub fn insert_rect(&mut self, rect: GridRect) {
        for v in rect.min.y..=rect.max.y {
            for u in rect.min.x..=rect.max.x {
                self.cells.insert((u, v));
            }
        }
    }

    pub fn insert(&mut self, cell: LocalCell) {
        self.cells.insert((cell.u, cell.v));
    }

    pub fn contains(&self, cell: LocalCell) -> bool {
        self.cells.contains(&(cell.u, cell.v))
    }

    /// Iterate in canonical `(u, v)` order, independent of insertion order.
    pub fn cells(&self) -> impl Iterator<Item = LocalCell> + '_ {
        self.cells.iter().map(|&(u, v)| LocalCell::new(u, v, 0))
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;

    #[test]
    fn rectangle_union_is_stable_and_deduplicated() {
        let mut a = FootprintMask::new();
        a.insert_rect(GridRect::new(Position::new(0, 0), Position::new(2, 1)));
        a.insert_rect(GridRect::new(Position::new(2, 0), Position::new(3, 1)));

        let mut b = FootprintMask::new();
        b.insert_rect(GridRect::new(Position::new(2, 0), Position::new(3, 1)));
        b.insert_rect(GridRect::new(Position::new(0, 0), Position::new(2, 1)));

        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert!(a.contains(LocalCell::new(3, 1, 0)));
        assert!(!a.contains(LocalCell::new(4, 1, 0)));
    }
}
