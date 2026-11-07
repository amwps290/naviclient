//! The shared cover column grid.

pub(in crate::app) const GRID_MIN_COLUMN: f32 = 160.0;
pub(in crate::app) const GRID_GAP: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::app) struct GridColumns {
    pub(in crate::app) count: usize,
    pub(in crate::app) width: f32,
}

/// As many columns as fit at the minimum width, stretched to fill exactly, so
/// every shelf shares its left and right edges with the row above it.
pub(in crate::app) fn grid_columns(available: f32) -> GridColumns {
    let available = available.max(0.0);
    let count = (((available + GRID_GAP) / (GRID_MIN_COLUMN + GRID_GAP)).floor() as usize).max(2);
    let width = ((available - (count as f32 - 1.0) * GRID_GAP) / count as f32).max(0.0);
    GridColumns { count, width }
}

#[cfg(test)]
mod tests;
