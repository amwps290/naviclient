//! Playback mode upgrade shared by global preferences and saved sessions.

use crate::models::{PlayModes, PlaybackMode};

/// 将旧版单枚举 `playback_mode` 迁移为二维 `playback_modes`
/// （随机开关 + 循环模式）。已有 `playback_modes` 或字段无法解析时不覆盖。
pub(super) fn migrate_legacy_playback_mode(value: &mut serde_json::Value) {
    if value.get("playback_modes").is_some() {
        return;
    }
    let Some(legacy) = value
        .get("playback_mode")
        .cloned()
        .and_then(|value| serde_json::from_value::<PlaybackMode>(value).ok())
    else {
        return;
    };
    if let Ok(modes) = serde_json::to_value(PlayModes::from_legacy(legacy)) {
        value["playback_modes"] = modes;
    }
}

#[cfg(test)]
mod tests;
