use super::*;

fn input(width: f32) -> ShellInput {
    ShellInput {
        viewport_width: width,
        window_padding: 0.0,
        sidebar_preference_collapsed: false,
        queue_open: false,
    }
}

#[test]
fn regular_window_uses_full_sidebar_and_page_padding() {
    let layout = shell_layout(input(1280.0));
    assert!(!layout.compact);
    assert!(!layout.sidebar_collapsed);
    assert_eq!(layout.sidebar_width, 200.0);
    assert_eq!(layout.page_padding, 32.0);
    assert_eq!(layout.player_margin, 12.0);
    assert_eq!(layout.content_width, 1280.0 - 200.0 - 64.0);
}

#[test]
fn compact_window_auto_collapses_the_sidebar() {
    let layout = shell_layout(input(1039.0));
    assert!(layout.compact);
    assert!(layout.auto_collapsed);
    assert_eq!(layout.sidebar_width, 56.0);
    assert_eq!(layout.page_padding, 20.0);
    assert_eq!(layout.player_margin, 8.0);
    assert_eq!(layout.queue_drawer_width, 320.0);
    assert_eq!(layout.content_width, 1039.0 - 56.0 - 40.0);
}

#[test]
fn threshold_width_is_regular() {
    assert!(!shell_layout(input(1040.0)).compact);
}

#[test]
fn collapsed_preference_is_not_auto_collapse() {
    let layout = shell_layout(ShellInput {
        sidebar_preference_collapsed: true,
        ..input(1280.0)
    });
    assert!(layout.sidebar_collapsed);
    assert!(!layout.auto_collapsed);
    assert_eq!(layout.content_width, 1280.0 - 56.0 - 64.0);
}

#[test]
fn open_queue_drawer_takes_width_and_margin_from_content() {
    let layout = shell_layout(ShellInput {
        queue_open: true,
        ..input(1280.0)
    });
    assert_eq!(layout.content_width, 1280.0 - 200.0 - 380.0 - 12.0 - 64.0);
}

#[test]
fn window_shadow_padding_is_excluded() {
    let layout = shell_layout(ShellInput {
        window_padding: 24.0,
        ..input(1280.0)
    });
    assert_eq!(layout.content_width, 1280.0 - 24.0 - 200.0 - 64.0);
}

#[test]
fn absurdly_small_window_never_goes_negative() {
    assert_eq!(shell_layout(input(10.0)).content_width, 0.0);
}
