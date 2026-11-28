//! Queue edits, explicit playback actions, and invalidation of prepared playback.

use super::edits::QueueRemoval;
use super::logic::ShuffleHistory;
use super::source_queue::SourceQueueEdit;
use super::undo::{QueueUndoKind, QueueUndoRecord};
use crate::app::playback::occurrence::Selection;
use crate::app::playback::start::StartTarget;
use crate::app::prelude::*;
use crate::playback_timeline::PlaybackTimeline;

impl NavidromeApp {
    /// 将一组歌曲按块插入到当前播放歌曲之后；未在播放时插入队首。
    /// 必须按块插入：逐首调用 [`Self::insert_next`] 会把每首都插到
    /// 同一位置导致顺序倒置。随机播放历史一次性平移插入点之后的索引。
    pub(in crate::app) fn insert_next_many(&mut self, songs: Vec<Song>) {
        if songs.is_empty() {
            return;
        }
        let Some(songs) = self.route_queue_edit(songs, SourceQueueEdit::InsertNext) else {
            return;
        };
        self.prepare_manual_queue_change();
        if self.state.edit_queue().insert_next(songs) {
            self.save_session();
        }
    }

    pub(in crate::app) fn replace_queue_and_play(
        &mut self,
        songs: Vec<Song>,
        index: usize,
        origin: QueueOrigin,
    ) -> bool {
        let queue = PlaybackQueue::from_songs(songs, origin);
        let Some(target) = StartTarget::replacement(queue, index) else {
            return false;
        };
        self.start_queue_replacement(target, index)
    }

    pub(in crate::app) fn replace_queue_and_play_mixed(
        &mut self,
        songs: Vec<Song>,
        index: usize,
        origin: QueueOrigin,
    ) -> bool {
        let queue = PlaybackQueue::from_songs(songs, origin);
        let Some(target) = StartTarget::mixed_replacement(queue, index) else {
            return false;
        };
        self.start_queue_replacement(target, index)
    }

    fn start_queue_replacement(&mut self, target: StartTarget, index: usize) -> bool {
        let Some(start) = self.prepare_playback_start(target, Duration::ZERO, Selection::New)
        else {
            return false;
        };
        let mut history = ShuffleHistory::default();
        history.start(index);
        self.prepare_manual_queue_change();
        if !self.activate_playback_start(start.with_history(history)) {
            return false;
        }
        self.state.queue_undo.clear();
        self.save_session();
        true
    }

    /// 将单首歌曲插入到当前播放歌曲之后；未在播放时插入队首（不自动播放）。
    pub(in crate::app) fn insert_next(&mut self, song: Song) {
        self.insert_next_many(vec![song]);
    }

    /// 将单首歌曲追加到队列末尾（不自动播放；空队列时设为队首索引以便播放键可用）。
    pub(in crate::app) fn add_to_queue(&mut self, song: Song) {
        self.append_all_with_origin(vec![song], QueueOrigin::Manual);
    }

    /// 追加整个列表到队列末尾（不自动播放）。
    pub(in crate::app) fn append_all(&mut self, songs: Vec<Song>) {
        self.append_all_with_origin(songs, QueueOrigin::Manual);
    }

    pub(in crate::app) fn append_all_with_origin(&mut self, songs: Vec<Song>, origin: QueueOrigin) {
        if songs.is_empty() {
            return;
        }
        let Some(songs) = self.route_queue_edit(songs, SourceQueueEdit::Append(origin)) else {
            return;
        };
        self.prepare_manual_queue_change();
        if self.state.edit_queue().append(songs, origin) {
            self.save_session();
        }
    }

    fn record_queue_undo(&mut self, kind: QueueUndoKind, displaced_current: bool) {
        let resume_position = displaced_current
            .then(|| self.playback_timeline().map(|timeline| timeline.position))
            .flatten();
        self.state.queue_undo.record(QueueUndoRecord {
            source: self.session.source().cloned(),
            queue: &self.state.queue,
            queue_index: self.state.queue_index,
            shuffle_history: &self.state.shuffle_history,
            kind,
            displaced_current,
            resume_position,
            now: std::time::Instant::now(),
        });
    }

    fn apply_queue_removal(&mut self, removal: QueueRemoval) {
        match removal {
            QueueRemoval::Missing | QueueRemoval::KeptCurrent => {}
            QueueRemoval::Stop => self.stop_playback(),
            QueueRemoval::PlayReplacement(next) => {
                let mut history = self.state.shuffle_history.clone();
                history.advance(next);
                if !self.play_queue_index_with_history(next, history) {
                    self.stop_playback();
                }
            }
        }
    }

    /// 从队列移除指定项并修正当前索引。删除正在播放的歌曲时改播原位置的下一首；队列清空则停止播放。
    pub(in crate::app) fn remove_from_queue(&mut self, index: usize) {
        if index >= self.state.queue.len() {
            return;
        }
        let displaced_current = self.state.queue_index == Some(index);
        self.record_queue_undo(QueueUndoKind::Removed(1), displaced_current);
        self.prepare_manual_queue_change();
        let removal = self.state.edit_queue().remove(index);
        self.apply_queue_removal(removal);
        self.save_session();
    }

    /// 从队列批量移除所选索引（去重后一次性重映射随机历史与选区）。
    pub(in crate::app) fn remove_many_from_queue(&mut self, indices: &[usize]) {
        let valid: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|index| *index < self.state.queue.len())
            .collect();
        if valid.is_empty() {
            return;
        }
        let displaced_current = self
            .state
            .queue_index
            .is_some_and(|current| valid.contains(&current));
        self.record_queue_undo(QueueUndoKind::Removed(valid.len()), displaced_current);
        self.prepare_manual_queue_change();
        let removal = self.state.edit_queue().remove_many(&valid);
        self.apply_queue_removal(removal);
        self.save_session();
    }

    /// 移除队列作用域中当前多选或键盘光标所在的条目。
    pub(in crate::app) fn remove_selected_from_queue(&mut self) -> bool {
        let len = self.state.queue.len();
        let mut indices: Vec<usize> = self
            .state
            .song_selection
            .selected_indices(SongScope::Queue, len)
            .collect();
        if indices.is_empty() {
            if let Some((SongScope::Queue, cursor)) = self.state.song_selection.cursor() {
                if cursor < len {
                    indices.push(cursor);
                }
            }
        }
        if indices.is_empty() {
            return false;
        }
        self.remove_many_from_queue(&indices);
        true
    }

    /// 清空当前播放歌曲之后的所有队列项。
    pub(in crate::app) fn clear_queue_after_current(&mut self) {
        let has_after = self
            .state
            .queue_index
            .is_some_and(|current| current + 1 < self.state.queue.len());
        if !has_after {
            return;
        }
        self.record_queue_undo(QueueUndoKind::ClearedAfter, false);
        self.prepare_manual_queue_change();
        if self.state.edit_queue().trim_after_current() {
            self.save_session();
        }
    }

    /// 清空整个队列并停止播放。
    pub(in crate::app) fn clear_queue(&mut self) {
        if self.state.queue.is_empty() {
            return;
        }
        self.record_queue_undo(QueueUndoKind::ClearedAll, self.state.queue_index.is_some());
        self.prepare_manual_queue_change();
        self.state.edit_queue().clear();
        self.stop_playback();
        self.save_session();
    }

    /// 将队列项从 `from` 移动到 `to`，并修正当前索引。
    pub(in crate::app) fn move_queue_item(&mut self, from: usize, to: usize) {
        if from >= self.state.queue.len() || to >= self.state.queue.len() || from == to {
            return;
        }
        self.record_queue_undo(QueueUndoKind::Reordered, false);
        self.prepare_manual_queue_change();
        if !self.state.edit_queue().move_entry(from, to) {
            return;
        }
        self.save_session();
    }

    /// 撤销最近一次队列破坏性编辑或重排；若当前歌曲未被移除则完全不打断正在播放的音频。
    pub(in crate::app) fn undo_queue_edit(&mut self) -> bool {
        let Some(entry) = self
            .state
            .queue_undo
            .take(std::time::Instant::now(), self.session.source())
        else {
            return false;
        };
        self.prepare_manual_queue_change();
        self.state.song_selection.clear_scope(SongScope::Queue);
        self.state.queue = entry.queue;
        self.state.queue_index = entry.queue_index;
        self.state.shuffle_history = entry.shuffle_history;

        if entry.displaced_current {
            // Stop any replacement track and restore the previous song paused at its prior position.
            self.stop_playback();
            if let Some(index) = self.state.queue_index {
                if let Some((entry_id, song)) = self
                    .state
                    .queue
                    .entry_id(index)
                    .zip(self.state.queue.song(index).cloned())
                {
                    self.state.now_playing = Some(song.clone());
                    self.state.occurrence.restore(entry_id, &song.id);
                    if let Some(position) = entry.resume_position {
                        let timeline = PlaybackTimeline::for_song(&PlaybackState::default(), &song)
                            .with_position(position);
                        self.session.set_position(timeline);
                    }
                    self.ensure_song_cover(&song, true);
                    self.load_lyrics(&song);
                }
            }
        }
        self.save_session();
        true
    }

    pub(in crate::app) fn scroll_queue_to_current(&self) {
        if let Some(index) = self.state.queue_index {
            if index < self.state.queue.len() {
                self.song_list_scroll(SongScope::Queue)
                    .scroll_to_item(index, gpui_kit::ScrollStrategy::Center);
            }
        }
    }

    pub(in crate::app) fn prepare_manual_queue_change(&mut self) {
        self.start_resolution.cancel();
        self.cancel_recommendation_tasks();
        self.cancel_next_preload();
    }
}
