use super::*;

#[test]
fn matches_the_spec_reference_widths() {
    let narrow = grid_columns(800.0);
    assert_eq!(narrow.count, 4);
    assert!((narrow.width - 185.0).abs() < 0.01);

    let regular = grid_columns(1016.0);
    assert_eq!(regular.count, 5);
    assert!((regular.width - 187.2).abs() < 0.01);
}

#[test]
fn columns_always_fill_the_available_width_exactly() {
    for available in [400.0, 777.0, 1016.0, 1600.0, 3000.0, 7000.0] {
        let grid = grid_columns(available);
        let used = grid.width * grid.count as f32 + GRID_GAP * (grid.count as f32 - 1.0);
        assert!((used - available).abs() < 0.01, "{available}");
        assert!(grid.width >= GRID_MIN_COLUMN, "{available}");
    }
}

#[test]
fn wider_windows_show_more_covers_without_a_cap() {
    assert!(grid_columns(3000.0).count > grid_columns(1600.0).count);
    assert!(grid_columns(7000.0).count > grid_columns(3000.0).count);
}

#[test]
fn tiny_and_zero_widths_keep_two_columns_without_negative_sizes() {
    assert_eq!(grid_columns(0.0).count, 2);
    assert_eq!(grid_columns(0.0).width, 0.0);
    assert_eq!(grid_columns(100.0).count, 2);
    assert_eq!(grid_columns(100.0).width, 40.0);
    assert_eq!(grid_columns(-50.0).width, 0.0);
}
