//! 播放队列操作与歌曲列表：作用域解析、键盘选择、增删移动、
//! 点击播放与按需预取封面。

use super::*;

impl NavidromeApp {
    /// 歌曲列表作用域对应的歌曲切片；列表滚动 / 事件处理时重新解析，
    /// 避免在每次渲染时克隆整个列表。
    pub(super) fn songs_for_scope(&self, scope: &str) -> Option<&[Song]> {
        match scope {
            "album-detail" => Some(&self.state.current_songs),
            "playlist-detail" => Some(&self.state.playlist_songs),
            "favorites" => Some(&self.state.favorites.songs),
            "search" => Some(&self.state.search_results.as_ref()?.songs),
            "queue" => Some(&self.state.queue),
            _ => None,
        }
    }

    pub(super) fn song_for_scope(&self, scope: &str, index: usize) -> Option<&Song> {
        self.songs_for_scope(scope)?.get(index)
    }

    /// 点击作用域列表的第 index 行：从当前列表重建队列并从该行开始播放。
    /// 队列作用域只定位到该队列位置，不重建队列。
    pub(super) fn play_song_at_scope(&mut self, scope: &str, index: usize) {
        let Some(songs) = self.songs_for_scope(scope) else {
            return;
        };
        if index >= songs.len() {
            return;
        }
        let replacement = if scope == "queue" {
            None
        } else {
            Some(songs.to_vec())
        };
        self.cancel_next_preload();
        if let Some(songs) = replacement {
            self.state.queue = songs;
        }
        self.state.shuffle_history.start(index);
        self.play_queue_index(index);
        self.save_session();
    }

    /// 取得（或首次创建）某作用域歌曲列表的滚动句柄。
    pub(super) fn song_list_scroll(&self, scope: &'static str) -> UniformListScrollHandle {
        self.song_list_scrolls
            .borrow_mut()
            .entry(scope)
            .or_default()
            .clone()
    }

    /// 歌曲列表滚动时按需加载可见行的封面。
    /// 当前页面主歌曲列表对应的作用域；无列表页面返回 None。
    pub(super) fn active_song_list_scope(&self) -> Option<&'static str> {
        match self.state.view {
            View::AlbumDetail => Some("album-detail"),
            View::PlaylistDetail => Some("playlist-detail"),
            View::Queue => Some("queue"),
            View::Favorites => Some("favorites"),
            View::Search => Some("search"),
            _ => None,
        }
    }

    /// Shift+↑/↓ 移动键盘选中行；无选中时从首行/末行开始。
    /// 自动滚动到选中行。
    pub(super) fn move_song_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.state.settings_open {
            return;
        }
        let Some(scope) = self.active_song_list_scope() else {
            return;
        };
        let len = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        if len == 0 {
            return;
        }
        let next = match self.state.song_list_selected {
            Some((selected_scope, index)) if selected_scope == scope => {
                index.saturating_add_signed(delta).min(len - 1)
            }
            _ => {
                if delta > 0 {
                    0
                } else {
                    len - 1
                }
            }
        };
        self.state.song_list_selected = Some((scope, next));
        self.song_list_scroll(scope)
            .scroll_to_item(next, gpui_kit::ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Enter 播放键盘选中的行；多选活跃时改为播放整个所选集合（从所选第一首开始）。
    pub(super) fn play_selected_song(&mut self, cx: &mut Context<Self>) {
        if self.state.settings_open {
            return;
        }
        if let Some(scope) = self.active_song_list_scope() {
            let songs = self.selected_songs(scope);
            if !songs.is_empty() {
                self.play_song_list(&songs, 0);
                cx.notify();
                return;
            }
        }
        let Some((scope, index)) = self.state.song_list_selected else {
            return;
        };
        if self.active_song_list_scope() != Some(scope) {
            return;
        }
        let len = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        if index < len {
            self.play_song_at_scope(scope, index);
        }
        cx.notify();
    }

    /// Ctrl+点击：切换该行选中态并把光标移到该行；集合收空时退出多选。
    pub(super) fn toggle_song_multi(&mut self, scope: &'static str, index: usize) {
        let mut set = match &self.state.song_list_multi {
            Some((selected_scope, set)) if *selected_scope == scope => set.clone(),
            _ => std::collections::BTreeSet::new(),
        };
        if !set.remove(&index) {
            set.insert(index);
        }
        self.state.song_list_selected = Some((scope, index));
        self.state.song_list_multi = (!set.is_empty()).then_some((scope, set));
    }

    /// Shift+点击：从光标（锚点）到点击行做范围选择；无光标时锚点即点击行。
    pub(super) fn range_song_multi(&mut self, scope: &'static str, index: usize) {
        let anchor = match self.state.song_list_selected {
            Some((selected_scope, selected_index)) if selected_scope == scope => selected_index,
            _ => index,
        };
        self.state.song_list_selected = Some((scope, index));
        self.state.song_list_multi = Some((scope, selection_range(anchor, index)));
    }

    /// Ctrl+A：全选当前作用域的全部歌曲。
    pub(super) fn select_all_songs(&mut self, cx: &mut Context<Self>) {
        if self.state.settings_open {
            return;
        }
        let Some(scope) = self.active_song_list_scope() else {
            return;
        };
        let len = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        if len == 0 {
            return;
        }
        let set = (0..len).collect();
        self.state.song_list_multi = Some((scope, set));
        cx.notify();
    }

    /// 清空多选集合（列表数据变化处调用，避免索引指向错误行）。
    pub(super) fn clear_song_multi(&mut self) {
        self.state.song_list_multi = None;
    }

    /// 当前作用域多选中有效的歌曲（按行索引升序）；scope 不匹配或
    /// 索引越界（列表已缩短）时被过滤，集合为空则返回空列表。
    pub(super) fn selected_songs(&self, scope: &str) -> Vec<Song> {
        let Some((selected_scope, indices)) = &self.state.song_list_multi else {
            return Vec::new();
        };
        if *selected_scope != scope {
            return Vec::new();
        }
        let Some(songs) = self.songs_for_scope(scope) else {
            return Vec::new();
        };
        indices
            .iter()
            .filter_map(|index| songs.get(*index).cloned())
            .collect()
    }

    /// 操作条显示用：当前作用域有效选中数量（过滤越界索引）。
    pub(super) fn multi_selection_count(&self, scope: &str) -> usize {
        self.selected_songs(scope).len()
    }

    /// 播放所选集合（从所选第一首开始，顺序保持所选升序）。
    /// 队列作用域与单曲点击语义一致：只定位到所选第一首的位置，
    /// 不用所选子集重建队列。
    pub(super) fn play_selected_songs(&mut self, scope: &str) {
        if scope == "queue" {
            if let Some(index) = self.first_selected_index(scope) {
                self.cancel_next_preload();
                self.state.shuffle_history.start(index);
                self.play_queue_index(index);
                self.save_session();
            }
            return;
        }
        let songs = self.selected_songs(scope);
        if !songs.is_empty() {
            self.play_song_list(&songs, 0);
        }
    }

    /// 所选集合的第一行索引（集合升序，越界索引已过滤）。
    pub(super) fn first_selected_index(&self, scope: &str) -> Option<usize> {
        let Some((selected_scope, indices)) = &self.state.song_list_multi else {
            return None;
        };
        if *selected_scope != scope {
            return None;
        }
        let len = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        indices.iter().copied().find(|index| *index < len)
    }

    /// 所选集合插入到当前播放歌曲之后，按块插入、一次历史重映射，保持所选顺序。
    pub(super) fn selected_play_next(&mut self, scope: &str) {
        let songs = self.selected_songs(scope);
        self.insert_next_many(&songs);
    }

    /// 所选集合追加到队列末尾（不自动播放）。
    pub(super) fn selected_to_queue(&mut self, scope: &str) {
        let songs = self.selected_songs(scope);
        self.append_all(&songs);
    }

    /// 将一组歌曲按块插入到当前播放歌曲之后；未在播放时插入队首。
    /// 必须按块插入：逐首调用 [`Self::insert_next`] 会把每首都插到
    /// 同一位置导致顺序倒置。随机播放历史一次性平移插入点之后的索引。
    pub(super) fn insert_next_many(&mut self, songs: &[Song]) {
        if songs.is_empty() {
            return;
        }
        self.cancel_next_preload();
        let insert_at = match self.state.queue_index {
            Some(index) => (index + 1).min(self.state.queue.len()),
            None => 0,
        };
        let count = songs.len();
        let mut tail = self.state.queue.split_off(insert_at);
        let mut block = songs.to_vec();
        block.append(&mut tail);
        self.state.queue.append(&mut block);
        self.state
            .shuffle_history
            .remap(|i| Some(history_index_after_insert(i, insert_at, count)));
        self.save_session();
    }

    /// 批量收藏所选歌曲：全部已收藏则统一取消，否则统一收藏。
    pub(super) fn favorite_selected(&mut self, scope: &str) {
        let songs = self.selected_songs(scope);
        if songs.is_empty() {
            return;
        }
        let keys: Vec<FavoriteKey> = songs
            .iter()
            .map(|song| FavoriteKey::new(FavoriteKind::Song, &song.id))
            .collect();
        let remove = keys.iter().all(|key| self.state.favorite_ids.contains(key));
        for key in keys {
            if self.state.favorite_ids.contains(&key) == remove {
                self.toggle_favorite(key);
            }
        }
    }

    pub(super) fn maybe_load_visible_song_covers(&mut self) {
        let Some(scope) = self.active_song_list_scope() else {
            return;
        };
        let Some(songs) = self.songs_for_scope(scope) else {
            return;
        };
        if songs.is_empty() {
            return;
        }
        let Some(handle) = self.song_list_scrolls.borrow().get(scope).cloned() else {
            return;
        };
        let base = handle.0.borrow().base_handle.clone();
        let offset = f32::from(-base.offset().y).max(0.0);
        let viewport = f32::from(base.bounds().size.height).max(0.0);
        // 额外预取约两个视口的封面。
        let row_height = SONG_ROW_HEIGHT;
        let first = ((offset / row_height) as usize).min(songs.len());
        let lookahead = ((viewport / row_height) as usize).saturating_mul(2).max(40);
        let last = first.saturating_add(lookahead).min(songs.len());
        // 先收集需要加载的封面 id，避免在借用期间修改 self。
        let pending: Vec<String> = songs[first..last]
            .iter()
            .filter_map(|song| song.cover_art.clone())
            .filter(|cover| !self.state.requested_covers.contains(cover))
            .collect();
        if pending.is_empty() {
            return;
        }
        log::debug!(
            "queued {} visible song covers; rows={first}..{last}",
            pending.len()
        );
        for cover in pending {
            self.ensure_cover(Some(&cover), false);
        }
    }

    pub(super) fn play_song_list(&mut self, songs: &[Song], index: usize) {
        self.cancel_next_preload();
        self.state.queue = songs.to_vec();
        self.state.shuffle_history.start(index);
        self.play_queue_index(index);
        self.save_session();
    }

    /// 将单首歌曲插入到当前播放歌曲之后；未在播放时插入队首（不自动播放）。
    pub(super) fn insert_next(&mut self, song: Song) {
        self.cancel_next_preload();
        let insert_at = match self.state.queue_index {
            Some(index) => (index + 1).min(self.state.queue.len()),
            None => 0,
        };
        self.state.queue.insert(insert_at, song);
        self.state
            .shuffle_history
            .remap(|i| Some(if i >= insert_at { i + 1 } else { i }));
        self.save_session();
    }

    /// 将单首歌曲追加到队列末尾（不自动播放；空队列时设为队首索引以便播放键可用）。
    pub(super) fn add_to_queue(&mut self, song: Song) {
        self.cancel_next_preload();
        if self.state.queue.is_empty() {
            self.state.queue_index = Some(0);
        }
        self.state.queue.push(song);
        self.save_session();
    }

    /// 追加整个列表到队列末尾（不自动播放）。
    pub(super) fn append_all(&mut self, songs: &[Song]) {
        if songs.is_empty() {
            return;
        }
        self.cancel_next_preload();
        if self.state.queue.is_empty() {
            self.state.queue_index = Some(0);
        }
        self.state.queue.extend(songs.iter().cloned());
        self.save_session();
    }

    /// 从队列移除指定项并修正当前索引。删除正在播放的歌曲时改播原位置的下一首；队列清空则停止播放。
    pub(super) fn remove_from_queue(&mut self, index: usize) {
        self.clear_song_multi();
        if index >= self.state.queue.len() {
            return;
        }
        self.cancel_next_preload();
        let removed_current = self.state.queue_index == Some(index);
        self.state.queue.remove(index);
        self.state.shuffle_history.remap(|i| {
            if i == index {
                None
            } else {
                Some(if i > index { i - 1 } else { i })
            }
        });
        self.state.queue_index =
            queue_index_after_remove(index, self.state.queue_index, self.state.queue.len());
        if removed_current {
            if self.state.queue.is_empty() {
                self.stop_playback();
            } else if let Some(next) = self.state.queue_index {
                self.state.shuffle_history.advance(next);
                self.play_queue_index(next);
            }
        }
        self.save_session();
    }

    /// 清空当前播放歌曲之后的所有队列项。
    pub(super) fn clear_queue_after_current(&mut self) {
        self.cancel_next_preload();
        if let Some(current) = self.state.queue_index {
            let keep = current + 1;
            if self.state.queue.len() > keep {
                self.state.queue.truncate(keep);
                self.state
                    .shuffle_history
                    .remap(|i| (i < keep).then_some(i));
                self.save_session();
            }
        }
    }

    /// 清空整个队列并停止播放。
    pub(super) fn clear_queue(&mut self) {
        self.cancel_next_preload();
        self.state.queue.clear();
        self.state.queue_index = None;
        self.state.shuffle_history = ShuffleHistory::default();
        self.stop_playback();
        self.save_session();
    }

    /// 将队列项从 `from` 移动到 `to`，并修正当前索引。
    pub(super) fn move_queue_item(&mut self, from: usize, to: usize) {
        self.clear_song_multi();
        self.cancel_next_preload();
        let len = self.state.queue.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let song = self.state.queue.remove(from);
        self.state.queue.insert(to, song);
        self.state
            .shuffle_history
            .remap(|i| Some(queue_index_after_move(from, to, i)));
        if let Some(current) = self.state.queue_index {
            self.state.queue_index = Some(queue_index_after_move(from, to, current));
        }
        self.save_session();
    }
}
