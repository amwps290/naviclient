//! Pure playback decisions: preloading and scrobbling.

use std::time::Duration;

use crate::audio::PlaybackState;
pub(super) fn should_preload_next(playback: &PlaybackState) -> bool {
    playback.active
        && !playback.paused
        && !playback.ended
        && playback.error.is_none()
        && playback.fully_buffered
        && playback.duration.is_some_and(|duration| {
            let remaining = duration.saturating_sub(playback.position);
            !remaining.is_zero() && remaining <= Duration::from_secs(15)
        })
}

/// 判断播放是否达到有效播放阈值（满足其一即可）：播放时长达到歌曲总长的 50%，或达到 4 分钟。
pub(super) fn should_scrobble(playback: &PlaybackState) -> bool {
    if !playback.active || playback.paused || playback.error.is_some() {
        return false;
    }
    let position = playback.position;
    let duration = playback.duration;
    if position.as_secs() >= 4 * 60 {
        return true;
    }
    match duration {
        Some(duration) if duration > Duration::ZERO => {
            position.as_secs_f64() >= duration.as_secs_f64() * 0.5
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
