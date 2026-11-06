use super::*;

#[test]
fn duplicate_items_have_distinct_hover_and_element_keys() {
    let first = super::item_instance_key("recent", "album", 0, "same-id");
    assert_ne!(
        first,
        super::item_instance_key("frequent", "album", 0, "same-id")
    );
    assert_ne!(
        first,
        super::item_instance_key("recent", "album", 1, "same-id")
    );
    assert_ne!(
        first,
        super::item_instance_key("recent", "song", 0, "same-id")
    );
    assert_ne!(
        super::item_instance_key("queue", "song", 0, "repeat"),
        super::item_instance_key("queue", "song", 1, "repeat")
    );
}

#[test]
fn selection_range_covers_anchor_to_clicked_in_order() {
    // 正向
    assert_eq!(
        selection_range(2, 5),
        [2, 3, 4, 5]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    // 反向（点击行在锚点之前）
    assert_eq!(
        selection_range(7, 3),
        [3, 4, 5, 6, 7]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    // 同一行
    assert_eq!(
        selection_range(4, 4),
        [4].into_iter().collect::<std::collections::BTreeSet<_>>()
    );
}

#[test]
fn range_anchor_prefers_multi_anchor_then_cursor_then_fallback() {
    // 已有多选锚点优先：Shift 操作可围绕固定锚点连续扩展。
    assert_eq!(range_anchor(Some(4), Some(9), 2), 4);
    // 无多选时用键盘光标（Shift+点击 从光标拉范围）。
    assert_eq!(range_anchor(None, Some(9), 2), 9);
    // 两者皆无（新开多选）：锚点即本次目标行。
    assert_eq!(range_anchor(None, None, 2), 2);
}

#[test]
fn song_rows_select_on_single_click_and_play_on_double_click() {
    assert_eq!(
        song_row_click_action(Modifiers::default(), 1),
        SongRowClickAction::Select
    );
    assert_eq!(
        song_row_click_action(Modifiers::default(), 2),
        SongRowClickAction::Play
    );
    assert_eq!(
        song_row_click_action(
            Modifiers {
                control: true,
                ..Modifiers::default()
            },
            1,
        ),
        SongRowClickAction::ToggleMulti
    );
    assert_eq!(
        song_row_click_action(
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
            1,
        ),
        SongRowClickAction::RangeMulti
    );
}

#[test]
fn compact_layout_breakpoints_preserve_regular_desktop_widths() {
    assert!(compact_window_layout(900.0));
    assert!(compact_window_layout(1039.0));
    assert!(!compact_window_layout(1040.0));
    assert!(!compact_song_columns(0.0));
    assert!(compact_song_columns(844.0));
    assert!(!compact_song_columns(900.0));
}
