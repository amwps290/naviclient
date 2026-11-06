//! Queue edits, explicit playback actions, and invalidation of prepared playback.

use super::playback_occurrence::Selection;
use super::playback_start::StartTarget;
use super::queue_edits::QueueRemoval;
use super::queue_logic::ShuffleHistory;
use super::*;

impl NavidromeApp {
    /// 将一组歌曲按块插入到当前播放歌曲之后；未在播放时插入队首。
    /// 必须按块插入：逐首调用 [`Self::insert_next`] 会把每首都插到
    /// 同一位置导致顺序倒置。随机播放历史一次性平移插入点之后的索引。
    pub(super) fn insert_next_many(&mut self, songs: Vec<Song>) {
        if songs.is_empty() {
            return;
        }
        self.prepare_manual_queue_change();
        if self.state.edit_queue().insert_next(songs) {
            self.save_session();
        }
    }

    pub(super) fn replace_queue_and_play(
        &mut self,
        songs: Vec<Song>,
        index: usize,
        origin: QueueOrigin,
    ) -> bool {
        let Some(target) =
            StartTarget::replacement(PlaybackQueue::from_songs(songs, origin), index)
        else {
            return false;
        };
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
        self.save_session();
        true
    }

    /// 将单首歌曲插入到当前播放歌曲之后；未在播放时插入队首（不自动播放）。
    pub(super) fn insert_next(&mut self, song: Song) {
        self.insert_next_many(vec![song]);
    }

    /// 将单首歌曲追加到队列末尾（不自动播放；空队列时设为队首索引以便播放键可用）。
    pub(super) fn add_to_queue(&mut self, song: Song) {
        self.append_all_with_origin(vec![song], QueueOrigin::Manual);
    }

    /// 追加整个列表到队列末尾（不自动播放）。
    pub(super) fn append_all(&mut self, songs: Vec<Song>) {
        self.append_all_with_origin(songs, QueueOrigin::Manual);
    }

    pub(super) fn append_all_with_origin(&mut self, songs: Vec<Song>, origin: QueueOrigin) {
        if songs.is_empty() {
            return;
        }
        self.prepare_manual_queue_change();
        if self.state.edit_queue().append(songs, origin) {
            self.save_session();
        }
    }

    /// 从队列移除指定项并修正当前索引。删除正在播放的歌曲时改播原位置的下一首；队列清空则停止播放。
    pub(super) fn remove_from_queue(&mut self, index: usize) {
        if index >= self.state.queue.len() {
            return;
        }
        self.prepare_manual_queue_change();
        match self.state.edit_queue().remove(index) {
            QueueRemoval::Missing => return,
            QueueRemoval::KeptCurrent => {}
            QueueRemoval::Stop => self.stop_playback(),
            QueueRemoval::PlayReplacement(next) => {
                let mut history = self.state.shuffle_history.clone();
                history.advance(next);
                if !self.play_queue_index_with_history(next, history) {
                    self.stop_playback();
                }
            }
        }
        self.save_session();
    }

    /// 清空当前播放歌曲之后的所有队列项。
    pub(super) fn clear_queue_after_current(&mut self) {
        self.prepare_manual_queue_change();
        if self.state.edit_queue().trim_after_current() {
            self.save_session();
        }
    }

    /// 清空整个队列并停止播放。
    pub(super) fn clear_queue(&mut self) {
        self.prepare_manual_queue_change();
        self.state.edit_queue().clear();
        self.stop_playback();
        self.save_session();
    }

    /// 将队列项从 `from` 移动到 `to`，并修正当前索引。
    pub(super) fn move_queue_item(&mut self, from: usize, to: usize) {
        self.prepare_manual_queue_change();
        if !self.state.edit_queue().move_entry(from, to) {
            return;
        }
        self.save_session();
    }

    pub(super) fn prepare_manual_queue_change(&mut self) {
        self.cancel_recommendation_tasks();
        self.cancel_next_preload();
    }
}
