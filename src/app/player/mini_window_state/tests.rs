use super::*;

fn main_size() -> Size<Pixels> {
    size(px(1100.0), px(760.0))
}

#[test]
fn the_main_window_has_no_mini_resize_or_restore_request() {
    let mut state = MiniWindowState::default();
    assert!(!state.is_mini());
    assert!(state.resize_target(main_size()).is_none());
    assert!(state.exit().is_none());
    assert!(!state.is_mini());
}

#[test]
fn entering_and_exiting_preserves_custom_main_window_geometry() {
    let mut state = MiniWindowState::default();
    let target = state.enter(main_size(), false, Edges::default()).unwrap();
    assert!(state.is_mini());
    assert_eq!(target, size(px(260.0), px(56.0)));
    assert!(state.resize_target(target).is_none());
    assert_eq!(
        state.exit().unwrap(),
        MainWindowGeometry {
            size: main_size(),
            maximized: false
        }
    );
    assert!(!state.is_mini());
}

#[test]
fn client_side_decoration_edges_are_added_to_the_content_size() {
    let mut state = MiniWindowState::default();
    let paddings = Edges {
        left: px(12.0),
        right: px(16.0),
        top: px(8.0),
        bottom: px(18.0),
    };
    let target = state.enter(main_size(), false, paddings).unwrap();
    assert_eq!(target, size(px(288.0), px(82.0)));
    assert_eq!(target.width - paddings.left - paddings.right, px(260.0));
    assert_eq!(target.height - paddings.top - paddings.bottom, px(56.0));
    assert_eq!(state.resize_target(main_size()), Some(target));
}

#[test]
fn repeated_entry_cannot_replace_the_saved_geometry_or_fixed_target() {
    let mut state = MiniWindowState::default();
    let target = state
        .enter(
            main_size(),
            true,
            Edges {
                left: px(12.0),
                right: px(12.0),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(state.enter(target, false, Edges::default()).is_none());
    assert_eq!(state.resize_target(main_size()), Some(target));
    assert_eq!(
        state.exit().unwrap(),
        MainWindowGeometry {
            size: main_size(),
            maximized: true
        }
    );
}

#[test]
fn a_restore_request_is_consumed_once_and_cannot_resize_the_main_window_again() {
    let mut state = MiniWindowState::default();
    let target = state.enter(main_size(), true, Edges::default()).unwrap();
    let restored = state.exit().unwrap();
    assert_eq!(restored.size, main_size());
    assert!(restored.maximized);
    assert!(state.exit().is_none());
    assert!(state.resize_target(target).is_none());
}

#[test]
fn separate_mini_sessions_capture_fresh_sizes_and_maximize_intent() {
    let mut state = MiniWindowState::default();
    for (width, height, maximized) in [
        (1920.0, 1080.0, true),
        (900.0, 600.0, false),
        (1440.0, 960.0, true),
        (1100.0, 760.0, false),
    ] {
        let viewport = size(px(width), px(height));
        assert!(state.enter(viewport, maximized, Edges::default()).is_some());
        assert_eq!(
            state.exit().unwrap(),
            MainWindowGeometry {
                size: viewport,
                maximized
            }
        );
        assert!(!state.is_mini());
    }
}

#[test]
fn size_correction_preserves_the_one_pixel_tolerance_on_each_axis() {
    let mut state = MiniWindowState::default();
    let target = state.enter(main_size(), false, Edges::default()).unwrap();
    for (width_delta, height_delta) in [
        (0.0, 0.0),
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (1.0, -1.0),
    ] {
        let viewport = size(
            target.width + px(width_delta),
            target.height + px(height_delta),
        );
        assert!(state.resize_target(viewport).is_none());
    }
    for (width_delta, height_delta) in [(1.25, 0.0), (-1.25, 0.0), (0.0, 1.25), (0.0, -1.25)] {
        let viewport = size(
            target.width + px(width_delta),
            target.height + px(height_delta),
        );
        assert_eq!(state.resize_target(viewport), Some(target));
    }
}

#[test]
fn correcting_mini_resizes_does_not_overwrite_the_main_restore_geometry() {
    let mut state = MiniWindowState::default();
    let target = state.enter(main_size(), true, Edges::default()).unwrap();
    for viewport in [
        size(px(800.0), px(400.0)),
        size(px(320.0), px(100.0)),
        main_size(),
    ] {
        assert_eq!(state.resize_target(viewport), Some(target));
    }
    assert_eq!(
        state.exit().unwrap(),
        MainWindowGeometry {
            size: main_size(),
            maximized: true
        }
    );
}

#[test]
fn fractional_decoration_sizes_preserve_the_drawable_content_dimensions() {
    let mut state = MiniWindowState::default();
    let paddings = Edges {
        left: px(7.25),
        right: px(7.5),
        top: px(3.25),
        bottom: px(4.5),
    };
    let target = state.enter(main_size(), false, paddings).unwrap();
    assert_eq!(target.width - paddings.left - paddings.right, px(260.0));
    assert_eq!(target.height - paddings.top - paddings.bottom, px(56.0));
    assert!(state.resize_target(target).is_none());
}
