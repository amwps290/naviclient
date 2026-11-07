//! Window layout thresholds, stable element keys, and song row selection.

use gpui_kit::Modifiers;
/// 歌曲行高。uniform_list 限高模式和封面预取都依赖它。
pub(in crate::app) const SONG_ROW_HEIGHT: f32 = 56.0;

pub(in crate::app) fn compact_window_layout(viewport_width: f32) -> bool {
    viewport_width < 1_040.0
}

pub(in crate::app) fn compact_song_columns(content_width: f32) -> bool {
    content_width > 0.0 && content_width < 900.0
}

pub(in crate::app) fn item_instance_key(scope: &str, kind: &str, index: usize, id: &str) -> String {
    format!("{scope}/{kind}/{index}/{id}")
}

/// Shift 范围选择锚点的优先级：已有同作用域多选的锚点（允许连续
/// 扩展）> 键盘光标 > 本次目标行。
pub(in crate::app) fn range_anchor(
    multi_anchor: Option<usize>,
    cursor: Option<usize>,
    fallback: usize,
) -> usize {
    multi_anchor.or(cursor).unwrap_or(fallback)
}

/// Shift+点击的范围选择集合（锚点到点击行的闭区间，升序）。
pub(in crate::app) fn selection_range(
    anchor: usize,
    clicked: usize,
) -> std::collections::BTreeSet<usize> {
    if anchor <= clicked {
        (anchor..=clicked).collect()
    } else {
        (clicked..=anchor).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum SongRowClickAction {
    Select,
    Play,
    ToggleMulti,
    RangeMulti,
}

/// 歌曲行遵循桌面媒体库语义：单击选择、双击播放；Ctrl/Shift 保留多选。
pub(in crate::app) fn song_row_click_action(
    modifiers: Modifiers,
    click_count: usize,
) -> SongRowClickAction {
    if modifiers.control && !modifiers.alt && !modifiers.platform {
        SongRowClickAction::ToggleMulti
    } else if modifiers.shift && !modifiers.alt && !modifiers.platform {
        SongRowClickAction::RangeMulti
    } else if click_count >= 2 {
        SongRowClickAction::Play
    } else {
        SongRowClickAction::Select
    }
}

#[cfg(test)]
mod tests;
