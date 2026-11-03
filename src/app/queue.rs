//! 播放队列操作与歌曲列表：作用域解析、键盘选择、增删移动、
//! 点击播放与按需预取封面。

use super::*;
use crate::recommendation::{generate_radio, RadioGenerationContext, RadioSeed};

/// Keep the legacy song/origin vectors aligned behind a small mutation API.
/// Read paths can stay unchanged while queue edits stop duplicating fragile
/// two-vector bookkeeping. This is also the migration seam for a future
/// `QueueEntry` representation.
impl AppState {
    fn replace_queue(&mut self, songs: Vec<Song>, origin: QueueOrigin) {
        self.queue_origins = vec![origin; songs.len()];
        self.queue = songs;
        self.assert_queue_alignment();
    }

    pub(super) fn replace_queue_with_origins(
        &mut self,
        songs: Vec<Song>,
        origins: Vec<QueueOrigin>,
    ) {
        self.queue_origins = normalize_queue_origins(origins, songs.len());
        self.queue = songs;
        self.assert_queue_alignment();
    }

    fn insert_queue_tracks(&mut self, index: usize, songs: &[Song], origin: QueueOrigin) {
        let index = index.min(self.queue.len());
        self.queue.splice(index..index, songs.iter().cloned());
        self.queue_origins
            .splice(index..index, std::iter::repeat_n(origin, songs.len()));
        self.assert_queue_alignment();
    }

    fn append_queue_tracks(&mut self, songs: &[Song], origin: QueueOrigin) {
        self.queue.extend(songs.iter().cloned());
        self.queue_origins
            .extend(std::iter::repeat_n(origin, songs.len()));
        self.assert_queue_alignment();
    }

    pub(super) fn append_queue_with_origins(
        &mut self,
        songs: Vec<Song>,
        origins: Vec<QueueOrigin>,
    ) {
        let origins = normalize_queue_origins(origins, songs.len());
        self.queue.extend(songs);
        self.queue_origins.extend(origins);
        self.assert_queue_alignment();
    }

    fn remove_queue_track(&mut self, index: usize) -> Option<(Song, QueueOrigin)> {
        if index >= self.queue.len() {
            return None;
        }
        let song = self.queue.remove(index);
        let origin = self.queue_origins.remove(index);
        self.assert_queue_alignment();
        Some((song, origin))
    }

    fn truncate_queue(&mut self, len: usize) {
        self.queue.truncate(len);
        self.queue_origins.truncate(len);
        self.assert_queue_alignment();
    }

    fn clear_queue_entries(&mut self) {
        self.queue.clear();
        self.queue_origins.clear();
        self.assert_queue_alignment();
    }

    fn move_queue_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.queue.len() || to >= self.queue.len() || from == to {
            return false;
        }
        let Some((song, origin)) = self.remove_queue_track(from) else {
            return false;
        };
        self.queue.insert(to, song);
        self.queue_origins.insert(to, origin);
        self.assert_queue_alignment();
        true
    }

    fn assert_queue_alignment(&self) {
        debug_assert_eq!(self.queue.len(), self.queue_origins.len());
    }
}

impl NavidromeApp {
    /// 歌曲列表作用域对应的歌曲切片；列表滚动 / 事件处理时重新解析，
    /// 避免在每次渲染时克隆整个列表。
    pub(super) fn songs_for_scope(&self, scope: &str) -> Option<&[Song]> {
        match scope {
            "album-detail" => Some(&self.state.current_songs),
            "artist-top" => Some(&self.state.artist_top_songs),
            "playlist-detail" => Some(&self.state.playlist_songs),
            "favorites" => Some(&self.state.favorites.songs),
            "search" => Some(&self.state.search_results.as_ref()?.songs),
            "queue" => Some(&self.state.queue),
            "history" => Some(self.state.playback_history.songs()),
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
        let (replacement, play_index) = match scope {
            "queue" => (None, index),
            "history" => (Some(vec![songs[index].clone()]), 0),
            _ => (Some(songs.to_vec()), index),
        };
        self.prepare_manual_queue_change();
        if let Some(songs) = replacement {
            let origin = Self::origin_for_scope(scope);
            self.state.replace_queue(songs, origin);
        }
        self.state.shuffle_history.start(play_index);
        self.play_queue_index(play_index);
        self.save_session();
    }

    /// 普通单击只移动列表光标，不中断当前播放；双击或 Enter 才播放。
    pub(super) fn select_song_row(&mut self, scope: &'static str, index: usize) {
        if index >= self.songs_for_scope(scope).map_or(0, <[Song]>::len) {
            return;
        }
        self.clear_song_multi();
        self.state.song_list_selected = Some((scope, index));
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
        if self.state.queue_drawer_open {
            return Some(match self.state.queue_drawer_tab {
                QueueDrawerTab::UpNext => "queue",
                QueueDrawerTab::History => "history",
            });
        }
        match self.state.view {
            View::ArtistDetail => Some("artist-top"),
            View::AlbumDetail => Some("album-detail"),
            View::PlaylistDetail => Some("playlist-detail"),
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
        // 多选活跃时 Shift+↑/↓ 从锚点重新计算范围（越过锚点再折回时
        // 自然收缩）；无多选时保持纯光标移动。
        if let Some(multi) = &mut self.state.song_list_multi {
            if multi.scope == scope {
                multi.indices = selection_range(multi.anchor, next);
            }
        }
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
                self.play_song_list_with_origin(&songs, 0, Self::origin_for_scope(scope));
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

    /// Ctrl+点击：切换该行选中态并把光标与锚点移到该行；集合收空时退出多选。
    pub(super) fn toggle_song_multi(&mut self, scope: &'static str, index: usize) {
        let mut set = match &self.state.song_list_multi {
            Some(multi) if multi.scope == scope => multi.indices.clone(),
            _ => std::collections::BTreeSet::new(),
        };
        if !set.remove(&index) {
            set.insert(index);
        }
        self.state.song_list_selected = Some((scope, index));
        self.state.song_list_multi = (!set.is_empty()).then_some(SongListMulti {
            scope,
            anchor: index,
            indices: set,
        });
    }

    /// Shift+点击：从锚点到点击行做范围选择。锚点优先级：已有同作用域
    /// 多选的锚点（可连续扩展）> 键盘光标 > 点击行本身。
    pub(super) fn range_song_multi(&mut self, scope: &'static str, index: usize) {
        let multi_anchor = match &self.state.song_list_multi {
            Some(multi) if multi.scope == scope => Some(multi.anchor),
            _ => None,
        };
        let cursor = match self.state.song_list_selected {
            Some((selected_scope, selected_index)) if selected_scope == scope => {
                Some(selected_index)
            }
            _ => None,
        };
        let anchor = range_anchor(multi_anchor, cursor, index);
        self.state.song_list_selected = Some((scope, index));
        self.state.song_list_multi = Some(SongListMulti {
            scope,
            anchor,
            indices: selection_range(anchor, index),
        });
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
        self.state.song_list_multi = Some(SongListMulti {
            scope,
            anchor: 0,
            indices: (0..len).collect(),
        });
        cx.notify();
    }

    /// 清空多选集合（列表数据变化处调用，避免索引指向错误行）。
    pub(super) fn clear_song_multi(&mut self) {
        self.state.song_list_multi = None;
    }

    /// 当前作用域多选中有效的歌曲（按行索引升序）；scope 不匹配或
    /// 索引越界（列表已缩短）时被过滤，集合为空则返回空列表。
    pub(super) fn selected_songs(&self, scope: &str) -> Vec<Song> {
        let Some(multi) = &self.state.song_list_multi else {
            return Vec::new();
        };
        if multi.scope != scope {
            return Vec::new();
        }
        let Some(songs) = self.songs_for_scope(scope) else {
            return Vec::new();
        };
        multi
            .indices
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
                self.prepare_manual_queue_change();
                self.state.shuffle_history.start(index);
                self.play_queue_index(index);
                self.save_session();
            }
            return;
        }
        let songs = self.selected_songs(scope);
        if !songs.is_empty() {
            self.play_song_list_with_origin(&songs, 0, Self::origin_for_scope(scope));
        }
    }

    /// 所选集合的第一行索引（集合升序，越界索引已过滤）。
    pub(super) fn first_selected_index(&self, scope: &str) -> Option<usize> {
        let Some(multi) = &self.state.song_list_multi else {
            return None;
        };
        if multi.scope != scope {
            return None;
        }
        let len = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        multi.indices.iter().copied().find(|index| *index < len)
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
        self.prepare_manual_queue_change();
        let insert_at = match self.state.queue_index {
            Some(index) => (index + 1).min(self.state.queue.len()),
            None => 0,
        };
        let count = songs.len();
        self.state
            .insert_queue_tracks(insert_at, songs, QueueOrigin::Manual);
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
        self.play_song_list_with_origin(songs, index, QueueOrigin::Manual);
    }

    pub(super) fn play_song_list_with_origin(
        &mut self,
        songs: &[Song],
        index: usize,
        origin: QueueOrigin,
    ) {
        self.prepare_manual_queue_change();
        self.state.replace_queue(songs.to_vec(), origin);
        self.state.shuffle_history.start(index);
        self.play_queue_index(index);
        self.save_session();
    }

    fn origin_for_scope(scope: &str) -> QueueOrigin {
        if scope == "playlist-detail" {
            QueueOrigin::Playlist
        } else {
            QueueOrigin::Manual
        }
    }

    /// 将单首歌曲插入到当前播放歌曲之后；未在播放时插入队首（不自动播放）。
    pub(super) fn insert_next(&mut self, song: Song) {
        self.prepare_manual_queue_change();
        let insert_at = match self.state.queue_index {
            Some(index) => (index + 1).min(self.state.queue.len()),
            None => 0,
        };
        self.state
            .insert_queue_tracks(insert_at, &[song], QueueOrigin::Manual);
        self.state
            .shuffle_history
            .remap(|i| Some(if i >= insert_at { i + 1 } else { i }));
        self.save_session();
    }

    /// 将单首歌曲追加到队列末尾（不自动播放；空队列时设为队首索引以便播放键可用）。
    pub(super) fn add_to_queue(&mut self, song: Song) {
        self.prepare_manual_queue_change();
        if self.state.queue.is_empty() {
            self.state.queue_index = Some(0);
        }
        self.state.append_queue_tracks(&[song], QueueOrigin::Manual);
        self.save_session();
    }

    /// 追加整个列表到队列末尾（不自动播放）。
    pub(super) fn append_all(&mut self, songs: &[Song]) {
        self.append_all_with_origin(songs, QueueOrigin::Manual);
    }

    pub(super) fn append_all_with_origin(&mut self, songs: &[Song], origin: QueueOrigin) {
        if songs.is_empty() {
            return;
        }
        self.prepare_manual_queue_change();
        if self.state.queue.is_empty() {
            self.state.queue_index = Some(0);
        }
        self.state.append_queue_tracks(songs, origin);
        self.save_session();
    }

    /// 从队列移除指定项并修正当前索引。删除正在播放的歌曲时改播原位置的下一首；队列清空则停止播放。
    pub(super) fn remove_from_queue(&mut self, index: usize) {
        self.clear_song_multi();
        if index >= self.state.queue.len() {
            return;
        }
        self.prepare_manual_queue_change();
        let removed_current = self.state.queue_index == Some(index);
        let Some((_song, _origin)) = self.state.remove_queue_track(index) else {
            return;
        };
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
        self.prepare_manual_queue_change();
        if let Some(current) = self.state.queue_index {
            let keep = current + 1;
            if self.state.queue.len() > keep {
                self.state.truncate_queue(keep);
                self.state
                    .shuffle_history
                    .remap(|i| (i < keep).then_some(i));
                self.save_session();
            }
        }
    }

    /// 清空整个队列并停止播放。
    pub(super) fn clear_queue(&mut self) {
        self.prepare_manual_queue_change();
        self.state.clear_queue_entries();
        self.state.queue_index = None;
        self.state.shuffle_history = ShuffleHistory::default();
        self.stop_playback();
        self.save_session();
    }

    /// 将队列项从 `from` 移动到 `to`，并修正当前索引。
    pub(super) fn move_queue_item(&mut self, from: usize, to: usize) {
        self.clear_song_multi();
        self.prepare_manual_queue_change();
        if !self.state.move_queue_entry(from, to) {
            return;
        }
        self.state
            .shuffle_history
            .remap(|i| Some(queue_index_after_move(from, to, i)));
        if let Some(current) = self.state.queue_index {
            self.state.queue_index = Some(queue_index_after_move(from, to, current));
        }
        self.save_session();
    }

    /// Start a song radio immediately with its seed while unified recommendation
    /// candidates are generated in the background.
    pub(super) fn start_song_radio(&mut self, seed: Song) {
        self.start_radio(
            RadioSeed::Song(Box::new(seed.clone())),
            seed,
            QueueOrigin::SongRadio,
        );
    }

    /// Artist radio has its own recommendation context. A loaded popular song
    /// starts immediately, but candidate scoring is based on the artist rather
    /// than treating that song as a song-radio shortcut.
    pub(super) fn start_artist_radio_from_seed(&mut self, artist: Artist, initial_song: Song) {
        self.start_radio(
            RadioSeed::Artist(artist),
            initial_song,
            QueueOrigin::ArtistRadio,
        );
    }

    fn start_radio(&mut self, seed: RadioSeed, initial_song: Song, origin: QueueOrigin) {
        let Some(api) = self.api.clone() else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        let Some(server_id) = self.config.active_server_id().map(str::to_string) else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        self.cancel_recommendation_tasks();
        self.radio_generation = self.radio_generation.wrapping_add(1);
        let generation = self.radio_generation;
        let key = seed.key();
        self.radio_loading_key = Some(key.clone());

        self.cancel_next_preload();
        self.state.replace_queue(vec![initial_song.clone()], origin);
        self.state.shuffle_history.start(0);
        // Explicitly starting a radio means "continue after this track".
        if self.state.playback_modes.repeat == RepeatMode::One {
            self.state.playback_modes.repeat = RepeatMode::Off;
            self.config.playback_modes = self.state.playback_modes;
            if let Err(error) = config::save(&self.config) {
                self.notify_error(t!(
                    "Playback mode save failed: %{error}",
                    error = format!("{error:#}")
                ));
            }
        }
        self.play_queue_index(0);
        self.save_session();

        let capabilities = self
            .state
            .server
            .as_ref()
            .map(|server| server.capabilities)
            .unwrap_or_default();
        let common = self.recommendation_generation_context(
            server_id.clone(),
            capabilities.sonic_similarity,
            50,
        );
        let tx = self.tx.clone();
        let message_server_id = server_id.clone();
        let message_key = key.clone();
        let message_initial_song_id = initial_song.id.clone();
        self.spawn_future(async move {
            let result = generate_radio(
                api,
                RadioGenerationContext {
                    common,
                    seed,
                    top_songs_by_artist_id: capabilities.top_songs_by_artist_id,
                },
            )
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::Radio {
                generation,
                server_id: message_server_id,
                key: message_key,
                initial_song_id: message_initial_song_id,
                result,
            });
        });
    }

    pub(super) fn song_radio_loading(&self, song_id: &str) -> bool {
        self.radio_loading_key
            .as_deref()
            .and_then(|key| key.strip_prefix("song:"))
            == Some(song_id)
    }

    pub(super) fn artist_radio_loading(&self, artist_id: &str) -> bool {
        self.radio_loading_key
            .as_deref()
            .and_then(|key| key.strip_prefix("artist:"))
            == Some(artist_id)
    }

    fn prepare_manual_queue_change(&mut self) {
        self.cancel_recommendation_tasks();
        self.cancel_next_preload();
    }

    pub(super) fn cancel_recommendation_tasks(&mut self) {
        self.cancel_song_radio();
        self.cancel_auto_continue();
        // Daily Mix may keep generating for its cache/card, but a manual queue
        // action must revoke the deferred "play when ready" intent.
        self.state.daily_mix_play_when_ready = false;
    }

    fn cancel_song_radio(&mut self) {
        if self.radio_loading_key.take().is_some() {
            self.radio_generation = self.radio_generation.wrapping_add(1);
        }
    }

    pub(super) fn finish_radio(
        &mut self,
        generation: u64,
        server_id: String,
        key: String,
        initial_song_id: String,
        result: Result<Vec<crate::recommendation::RecommendedTrack>, String>,
    ) {
        if generation != self.radio_generation
            || self.config.active_server_id() != Some(server_id.as_str())
            || self.radio_loading_key.as_deref() != Some(key.as_str())
        {
            return;
        }
        self.radio_loading_key = None;

        let seed_queue_untouched = self.state.queue.len() == 1
            && self
                .state
                .queue
                .first()
                .is_some_and(|song| song.id == initial_song_id)
            && self.state.queue_index == Some(0)
            && self
                .state
                .now_playing
                .as_ref()
                .is_some_and(|song| song.id == initial_song_id);
        if !seed_queue_untouched {
            return;
        }

        match result {
            Ok(tracks) if !tracks.is_empty() => {
                let count = tracks.len();
                self.state.append_queue_with_origins(
                    tracks.iter().map(|track| track.song.clone()).collect(),
                    tracks.iter().map(|track| track.origin).collect(),
                );
                self.save_session();
                if key.starts_with("artist:") {
                    self.notify_success(t!("Artist radio added %{count} tracks", count = count));
                } else {
                    self.notify_success(t!("Song radio added %{count} tracks", count = count));
                }
            }
            Ok(_) if key.starts_with("artist:") => {
                self.notify_warning(t!("No tracks were found for this artist radio"))
            }
            Ok(_) => self.notify_warning(t!("No related tracks were found for this song")),
            Err(error) if key.starts_with("artist:") => {
                self.notify_error(t!("Could not build artist radio: %{error}", error = error))
            }
            Err(error) => {
                self.notify_error(t!("Could not build song radio: %{error}", error = error))
            }
        }
    }
}

#[cfg(test)]
mod queue_state_tests {
    use super::*;

    fn song(id: &str) -> Song {
        Song {
            id: id.into(),
            title: id.into(),
            ..Song::default()
        }
    }

    #[test]
    fn queue_mutations_keep_songs_and_origins_aligned() {
        let mut state = AppState::default();
        state.replace_queue(vec![song("a"), song("b")], QueueOrigin::Playlist);
        state.insert_queue_tracks(1, &[song("x"), song("y")], QueueOrigin::Manual);
        state.append_queue_with_origins(
            vec![song("radio")],
            vec![QueueOrigin::SongRadio, QueueOrigin::DailyMix],
        );
        assert_eq!(state.queue.len(), state.queue_origins.len());
        assert_eq!(
            state
                .queue
                .iter()
                .map(|song| song.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "x", "y", "b", "radio"]
        );
        assert_eq!(state.queue_origins[4], QueueOrigin::SongRadio);

        assert!(state.move_queue_entry(4, 1));
        assert_eq!(state.queue[1].id, "radio");
        assert_eq!(state.queue_origins[1], QueueOrigin::SongRadio);
        assert_eq!(
            state
                .remove_queue_track(2)
                .map(|(song, origin)| (song.id, origin)),
            Some(("x".into(), QueueOrigin::Manual))
        );
        state.truncate_queue(2);
        assert_eq!(state.queue.len(), state.queue_origins.len());
        state.clear_queue_entries();
        assert!(state.queue.is_empty());
        assert!(state.queue_origins.is_empty());
    }
}
