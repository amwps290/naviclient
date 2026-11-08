//! Borrowed song projections, scoped selection, keyboard navigation and scrolling.

use crate::app::prelude::*;
use crate::app::shell::view_helpers::SONG_ROW_HEIGHT;
use crate::models::{QueueEntry, Song};

/// Reads regular lists and queue occurrences without copying songs for rendering.
#[derive(Clone, Copy)]
pub(in crate::app) enum SongList<'a> {
    Songs(&'a [Song]),
    Queue(&'a [QueueEntry]),
}

impl<'a> SongList<'a> {
    pub(in crate::app) fn len(self) -> usize {
        match self {
            Self::Songs(songs) => songs.len(),
            Self::Queue(entries) => entries.len(),
        }
    }

    pub(in crate::app) fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub(in crate::app) fn get(self, index: usize) -> Option<&'a Song> {
        match self {
            Self::Songs(songs) => songs.get(index),
            Self::Queue(entries) => entries.get(index).map(|entry| &entry.song),
        }
    }

    pub(in crate::app) fn last(self) -> Option<&'a Song> {
        self.len().checked_sub(1).and_then(|index| self.get(index))
    }

    pub(in crate::app) fn iter(
        self,
    ) -> impl DoubleEndedIterator<Item = &'a Song> + ExactSizeIterator {
        (0..self.len()).map(move |index| match self {
            Self::Songs(songs) => &songs[index],
            Self::Queue(entries) => &entries[index].song,
        })
    }

    pub(in crate::app) fn to_vec(self) -> Vec<Song> {
        self.iter().cloned().collect()
    }
}

impl std::ops::Index<usize> for SongList<'_> {
    type Output = Song;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Self::Songs(songs) => &songs[index],
            Self::Queue(entries) => &entries[index].song,
        }
    }
}

impl AppState {
    pub(in crate::app) fn clear_song_selection_for_scope(&mut self, scope: SongScope) {
        self.song_selection.clear_scope(scope);
    }

    pub(in crate::app) fn selected_song_iter(
        &self,
        scope: SongScope,
    ) -> impl Iterator<Item = &Song> {
        let songs = self.songs_for_scope(scope);
        let len = songs.map_or(0, SongList::len);
        self.song_selection
            .selected_indices(scope, len)
            .filter_map(move |index| songs?.get(index))
    }

    pub(in crate::app) fn selected_songs(&self, scope: SongScope) -> Vec<Song> {
        self.selected_song_iter(scope).cloned().collect()
    }

    pub(in crate::app) fn selected_song_ids(&self, scope: SongScope) -> Vec<String> {
        self.selected_song_iter(scope)
            .map(|song| song.id.clone())
            .collect()
    }

    pub(in crate::app) fn selection_summary(&self, scope: SongScope) -> SongSelectionSummary {
        let mut count = 0;
        let mut all_favorited = true;
        for song in self.selected_song_iter(scope) {
            count += 1;
            all_favorited = all_favorited
                && self
                    .favorites
                    .is_starred(&FavoriteKey::new(FavoriteKind::Song, &song.id));
        }
        SongSelectionSummary {
            count,
            all_favorited: count > 0 && all_favorited,
        }
    }

    /// 歌曲列表作用域对应的借用投影；列表滚动 / 事件处理时重新解析，
    /// 避免在每次渲染时克隆整个列表。
    pub(in crate::app) fn songs_for_scope(&self, scope: SongScope) -> Option<SongList<'_>> {
        match scope {
            SongScope::AlbumDetail => Some(SongList::Songs(self.album_details.songs())),
            SongScope::ArtistTop => Some(SongList::Songs(self.artist_details.top_songs())),
            SongScope::PlaylistDetail => Some(SongList::Songs(self.playlists.songs())),
            SongScope::Favorites => Some(SongList::Songs(&self.favorites.items().songs)),
            SongScope::Search => Some(SongList::Songs(&self.search.results()?.songs)),
            SongScope::Queue => Some(SongList::Queue(self.queue.entries())),
            SongScope::History => Some(SongList::Songs(self.listening.recent_songs())),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct SongSelectionSummary {
    pub(in crate::app) count: usize,
    pub(in crate::app) all_favorited: bool,
}

impl NavidromeApp {
    pub(in crate::app) fn song_for_scope(&self, scope: SongScope, index: usize) -> Option<&Song> {
        self.state.songs_for_scope(scope)?.get(index)
    }

    /// 普通单击只移动列表光标，不中断当前播放；双击或 Enter 才播放。
    pub(in crate::app) fn select_song_row(&mut self, scope: SongScope, index: usize) {
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        self.state.song_selection.select(scope, index, len);
    }

    /// 取得（或首次创建）某作用域歌曲列表的滚动句柄。
    pub(in crate::app) fn song_list_scroll(&self, scope: SongScope) -> UniformListScrollHandle {
        self.song_list_scrolls
            .borrow_mut()
            .entry(scope)
            .or_default()
            .clone()
    }

    /// 当前页面主歌曲列表对应的作用域；无列表页面返回 None。
    pub(in crate::app) fn active_song_list_scope(&self) -> Option<SongScope> {
        SongScope::active(
            self.state.view,
            self.state.queue_drawer_open,
            self.state.queue_drawer_tab,
        )
    }

    /// Shift+↑/↓ 移动键盘选中行；无选中时从首行/末行开始。
    /// 自动滚动到选中行。
    pub(in crate::app) fn move_song_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.state.settings.open {
            return;
        }
        let Some(scope) = self.active_song_list_scope() else {
            return;
        };
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        let Some(next) = self.state.song_selection.move_cursor(scope, delta, len) else {
            return;
        };
        self.song_list_scroll(scope)
            .scroll_to_item(next, gpui_kit::ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Enter 播放键盘选中的行；多选活跃时改为播放整个所选集合（从所选第一首开始）。
    pub(in crate::app) fn play_selected_song(&mut self, cx: &mut Context<Self>) {
        if self.state.settings.open {
            return;
        }
        if let Some(scope) = self.active_song_list_scope() {
            if self.first_selected_index(scope).is_some() {
                self.play_selected_songs(scope);
                cx.notify();
                return;
            }
        }
        let Some((scope, index)) = self.state.song_selection.cursor() else {
            return;
        };
        if self.active_song_list_scope() != Some(scope) {
            return;
        }
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        if index < len {
            self.play_song_at_scope(scope, index);
        }
        cx.notify();
    }

    /// Ctrl+点击：切换该行选中态并把光标与锚点移到该行；集合收空时退出多选。
    pub(in crate::app) fn toggle_song_multi(&mut self, scope: SongScope, index: usize) {
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        self.state.song_selection.toggle(scope, index, len);
    }

    /// Shift+点击：从锚点到点击行做范围选择。锚点优先级：已有同作用域
    /// 多选的锚点（可连续扩展）> 键盘光标 > 点击行本身。
    pub(in crate::app) fn range_song_multi(&mut self, scope: SongScope, index: usize) {
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        self.state.song_selection.range(scope, index, len);
    }

    /// Ctrl+A：全选当前作用域的全部歌曲。
    pub(in crate::app) fn select_all_songs(&mut self, cx: &mut Context<Self>) {
        if self.state.settings.open {
            return;
        }
        let Some(scope) = self.active_song_list_scope() else {
            return;
        };
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        self.state.song_selection.select_all(scope, len);
        cx.notify();
    }

    /// 清空多选集合（列表数据变化处调用，避免索引指向错误行）。
    pub(in crate::app) fn clear_song_multi(&mut self) {
        self.state.song_selection.clear_multi();
    }

    /// 当前作用域多选中有效的歌曲（按行索引升序）；scope 不匹配或
    /// 索引越界（列表已缩短）时被过滤，集合为空则返回空列表。
    pub(in crate::app) fn selected_songs(&self, scope: SongScope) -> Vec<Song> {
        self.state.selected_songs(scope)
    }

    /// 操作条显示用：当前作用域有效选中数量（过滤越界索引）。
    pub(in crate::app) fn multi_selection_count(&self, scope: SongScope) -> usize {
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        self.state
            .song_selection
            .selected_indices(scope, len)
            .count()
    }

    /// 所选集合的第一行索引（集合升序，越界索引已过滤）。
    pub(in crate::app) fn first_selected_index(&self, scope: SongScope) -> Option<usize> {
        let len = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        self.state
            .song_selection
            .selected_indices(scope, len)
            .next()
    }

    /// 批量收藏所选歌曲：全部已收藏则统一取消，否则统一收藏。
    pub(in crate::app) fn favorite_selected(&mut self, scope: SongScope) {
        let keys: Vec<FavoriteKey> = self
            .state
            .selected_song_iter(scope)
            .filter(|song| self.browsing_owns_song(song))
            .map(|song| FavoriteKey::new(FavoriteKind::Song, &song.id))
            .collect();
        let remove = keys.iter().all(|key| self.state.favorites.is_starred(key));
        for key in keys {
            if self.state.favorites.is_starred(&key) == remove {
                self.toggle_favorite(key);
            }
        }
    }

    /// 歌曲列表滚动时按需加载可见行的封面。
    pub(in crate::app) fn maybe_load_visible_song_covers(&mut self) {
        let Some(scope) = self.active_song_list_scope() else {
            return;
        };
        let Some(songs) = self.state.songs_for_scope(scope) else {
            return;
        };
        if songs.is_empty() {
            return;
        }
        let Some(handle) = self.song_list_scrolls.borrow().get(&scope).cloned() else {
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
        let pending: Vec<Song> = songs
            .iter()
            .skip(first)
            .take(last - first)
            .filter(|song| song.cover_art.is_some())
            .cloned()
            .collect();
        if pending.is_empty() {
            return;
        }
        log::debug!(
            "queued {} visible song covers; rows={first}..{last}",
            pending.len()
        );
        for song in pending {
            self.ensure_song_cover(&song, false);
        }
    }
}

#[cfg(test)]
mod tests;
