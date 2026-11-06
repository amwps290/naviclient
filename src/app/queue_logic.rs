//! Queue progression, shuffle history, and index remapping.

use super::song_list::SongList;

use crate::models::{PlayModes, RepeatMode};

#[derive(Clone, Debug, Default)]
pub(super) struct ShuffleHistory {
    pub(super) played: Vec<usize>,
    pub(super) forward: Vec<usize>,
}

impl ShuffleHistory {
    pub(super) fn start(&mut self, index: usize) {
        self.played = vec![index];
        self.forward.clear();
    }

    /// 前进到 next：丢弃回退暂存，记录已播放序列。
    pub(super) fn advance(&mut self, next: usize) {
        self.forward.clear();
        self.played.push(next);
    }

    /// 回退到上一首：返回真正播放过的上一首；无可回退时返回 None。
    pub(super) fn previous(&mut self) -> Option<usize> {
        if self.played.len() < 2 {
            return None;
        }
        let current = self.played.pop()?;
        self.forward.push(current);
        self.played.last().copied()
    }

    /// 回退后恢复前进：返回暂存的下一首；无暂存时返回 None。
    pub(super) fn restore_forward(&mut self) -> Option<usize> {
        let index = self.forward.pop()?;
        self.played.push(index);
        Some(index)
    }

    /// 最近播放过的 index（用于随机排除，避免短时重复）。
    pub(super) fn recent(&self, max: usize) -> impl Iterator<Item = usize> + '_ {
        self.played.iter().rev().take(max).copied()
    }

    /// 从持久化快照恢复。
    pub(super) fn restore(played: Vec<usize>, forward: Vec<usize>) -> Self {
        Self { played, forward }
    }

    /// 导出快照用于持久化。
    pub(super) fn snapshot(&self) -> (Vec<usize>, Vec<usize>) {
        (self.played.clone(), self.forward.clone())
    }

    pub(super) fn remap(&mut self, mut map: impl FnMut(usize) -> Option<usize>) {
        self.played = self.played.iter().copied().filter_map(&mut map).collect();
        self.forward = self.forward.iter().copied().filter_map(map).collect();
    }
}

pub(super) fn should_auto_continue(
    enabled: bool,
    modes: PlayModes,
    current: Option<usize>,
    queue_len: usize,
    natural_end: bool,
) -> bool {
    enabled
        && natural_end
        && !modes.shuffle
        && modes.repeat == RepeatMode::Off
        && queue_len > 0
        && current == Some(queue_len - 1)
}

pub(super) fn auto_continue_context_matches(
    enabled: bool,
    modes: PlayModes,
    current: Option<usize>,
    queue: SongList<'_>,
    expected_len: usize,
    anchor_song_id: &str,
) -> bool {
    queue.len() == expected_len
        && queue.last().map(|song| song.id.as_str()) == Some(anchor_song_id)
        && should_auto_continue(enabled, modes, current, queue.len(), true)
}

pub(super) fn next_queue_index(
    modes: PlayModes,
    current: usize,
    len: usize,
    natural_end: bool,
    shuffle_forward: Option<usize>,
    reserved: Option<usize>,
    random: impl FnOnce() -> usize,
) -> Option<usize> {
    if current >= len {
        return None;
    }
    if modes.shuffle {
        // 随机 + 单曲循环：自然结束重播当前，手动切歌仍随机。
        if natural_end && modes.repeat == RepeatMode::One {
            return Some(current);
        }
        return shuffle_forward
            .or(reserved)
            .or_else(|| Some(random()))
            .filter(|i| *i < len);
    }
    match modes.repeat {
        RepeatMode::One if natural_end => Some(current),
        RepeatMode::One | RepeatMode::All => advance_index(current, len, true, true),
        RepeatMode::Off => advance_index(current, len, true, false),
    }
}

/// 计算顺序（非随机）切歌的目标 index；`wraps` 为 true 时队列首尾循环，否则在边界停止。
pub(super) fn advance_index(
    current: usize,
    len: usize,
    forward: bool,
    wraps: bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if forward {
        if current + 1 < len {
            Some(current + 1)
        } else if wraps {
            Some(0)
        } else {
            None
        }
    } else if current > 0 {
        Some(current - 1)
    } else if wraps {
        Some(len - 1)
    } else {
        None
    }
}

/// 从队列移除 `index`（`len_after` 为删除后的长度）后，返回修正后的当前索引。
/// 删除正在播放的歌曲时，回落到原位置的下一首；队列被清空时返回 `None`。
pub(super) fn queue_index_after_remove(
    index: usize,
    current: Option<usize>,
    len_after: usize,
) -> Option<usize> {
    let current = current?;
    if len_after == 0 {
        return None;
    }
    if index < current {
        Some(current - 1)
    } else if index == current {
        Some(current.min(len_after - 1))
    } else {
        Some(current)
    }
}

/// 将队列项从 `from` 移到 `to` 后，返回修正后的当前索引。
pub(super) fn queue_index_after_move(from: usize, to: usize, current: usize) -> usize {
    if from == current {
        return to;
    }
    let current_after_remove = if from < current { current - 1 } else { current };
    if current_after_remove >= to {
        current_after_remove + 1
    } else {
        current_after_remove
    }
}

/// 块插入后随机播放历史索引的平移：插入点及之后平移 count，之前的
/// 保持不变。与逐首插入（count = 1）的语义一致。
pub(super) fn history_index_after_insert(index: usize, insert_at: usize, count: usize) -> usize {
    if index >= insert_at {
        index + count
    } else {
        index
    }
}

#[cfg(test)]
mod tests;
