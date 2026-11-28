//! Song context menus and occurrence-based playlist actions.

use crate::app::library::navigation::{album_for_song, artist_for_song};
use crate::app::prelude::*;
use crate::models::QueueEntryId;
use crate::playlist::{can_edit, song_ids, PlaylistEdit};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::Focusable;

#[derive(Clone)]
pub(in crate::app) struct SongMenuContext {
    song: Song,
    source: SongMenuSource,
}

#[derive(Clone)]
enum SongMenuSource {
    Queue(QueueEntryId),
    Library {
        songs: Arc<Vec<Song>>,
        index: usize,
        playlist: Option<Playlist>,
        playlist_target: Option<PlaylistMenuTarget>,
    },
}

#[derive(Clone, Copy)]
struct PlaylistMenuTarget {
    snapshot: Option<LibraryRequest>,
    complete: bool,
    view: Option<crate::app::PlaylistSearchRequest>,
    filtered: bool,
}

impl PlaylistMenuTarget {
    fn is_current(self, state: &AppState) -> bool {
        self.snapshot.is_some()
            && self.snapshot == state.playlists.snapshot()
            && self.view == state.playlists.search_identity()
            && state.playlists.search.ready()
            && (!self.filtered || state.playlists.detail_ready())
    }

    fn can_play(self, state: &AppState) -> bool {
        self.complete && self.is_current(state) && state.playlists.detail_ready()
    }
}

impl SongMenuSource {
    fn playlist_target(&self) -> Option<PlaylistMenuTarget> {
        match self {
            Self::Library {
                playlist_target, ..
            } => *playlist_target,
            Self::Queue(_) => None,
        }
    }

    fn can_play(&self, state: &AppState) -> bool {
        match self {
            Self::Queue(entry) => queue_menu_index(&state.queue, *entry).is_some(),
            Self::Library {
                playlist_target, ..
            } => playlist_target.is_none_or(|target| target.can_play(state)),
        }
    }
}

impl SongMenuContext {
    fn for_scope(state: &AppState, scope: SongScope, index: usize) -> Option<Self> {
        if scope == SongScope::PlaylistDetail
            && (!state.playlists.search.ready()
                || (state.playlists.search.filtered() && !state.playlists.detail_ready()))
        {
            return None;
        }
        let songs = state.songs_for_scope(scope)?;
        let song = songs.get(index)?.clone();
        let complete = state.song_scope_ready(scope);
        let source = if scope == SongScope::Queue {
            SongMenuSource::Queue(state.queue.entry_id(index)?)
        } else {
            SongMenuSource::Library {
                // Preview menus can operate on the captured song, but never
                // freeze a partial list into a future "play all" action.
                songs: Arc::new(if !complete {
                    Vec::new()
                } else if scope == SongScope::PlaylistDetail {
                    state.playlists.songs().to_vec()
                } else {
                    songs.to_vec()
                }),
                index: songs.original_index(index)?,
                playlist: (scope == SongScope::PlaylistDetail)
                    .then(|| state.playlists.playlist().cloned())
                    .flatten(),
                playlist_target: (scope == SongScope::PlaylistDetail).then_some(
                    PlaylistMenuTarget {
                        snapshot: state.playlists.snapshot(),
                        complete,
                        view: state.playlists.search_identity(),
                        filtered: state.playlists.search.filtered(),
                    },
                ),
            }
        };
        Some(Self { song, source })
    }
}

fn queue_menu_index(queue: &PlaybackQueue, entry: QueueEntryId) -> Option<usize> {
    queue.index_of(entry)
}

impl NavidromeApp {
    /// Capture the current song and its list or queue occurrence when opening.
    pub(in crate::app) fn song_menu_context(
        &self,
        scope: SongScope,
        index: usize,
    ) -> Option<SongMenuContext> {
        SongMenuContext::for_scope(&self.state, scope, index)
    }

    pub(in crate::app) fn song_menu(
        &mut self,
        mut menu: PopupMenu,
        context: SongMenuContext,
        cx: &Context<Self>,
    ) -> PopupMenu {
        self.playlist_ui.menu_focus = Some(menu.focus_handle(cx));
        let song = context.song;
        let browsing_owner = self.browsing_owns_song(&song);
        let capabilities = self
            .api
            .as_ref()
            .map(MusicSource::capabilities)
            .unwrap_or_default();
        let play = context.source.clone();
        let playback_allowed = play.can_play(&self.state);
        let playlist_target = play.playlist_target();
        menu = menu.item(
            PopupMenuItem::new(t!("Play now"))
                .disabled(!playback_allowed)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !play.can_play(&this.state) {
                        return;
                    }
                    match &play {
                        SongMenuSource::Queue(entry) => {
                            if let Some(index) = queue_menu_index(&this.state.queue, *entry) {
                                this.play_song_at_scope(SongScope::Queue, index);
                            }
                        }
                        SongMenuSource::Library {
                            songs,
                            index,
                            playlist,
                            ..
                        } => {
                            let origin = if playlist.is_some() {
                                QueueOrigin::Playlist
                            } else {
                                QueueOrigin::Manual
                            };
                            this.replace_queue_and_play(songs.as_ref().clone(), *index, origin);
                        }
                    }
                    cx.notify();
                })),
        );
        let next = song.clone();
        menu = menu.item(PopupMenuItem::new(t!("Play next")).on_click(cx.listener(
            move |this, _, _, cx| {
                if !playlist_target.is_none_or(|target| target.is_current(&this.state)) {
                    return;
                }
                this.insert_next(next.clone());
                cx.notify();
            },
        )));
        let append = song.clone();
        menu = menu.item(PopupMenuItem::new(t!("Add to queue")).on_click(cx.listener(
            move |this, _, _, cx| {
                if !playlist_target.is_none_or(|target| target.is_current(&this.state)) {
                    return;
                }
                this.add_to_queue(append.clone());
                cx.notify();
            },
        )));
        let is_bilibili = song
            .source
            .as_ref()
            .is_some_and(|source| source.provider == crate::source::ProviderKind::Bilibili);
        if let (Some(part), SongMenuSource::Library { songs, .. }) = (
            song.bilibili_part.as_ref().filter(|part| part.count > 1),
            &context.source,
        ) {
            let parts = songs
                .iter()
                .filter(|candidate| {
                    candidate.source == song.source
                        && candidate
                            .bilibili_part
                            .as_ref()
                            .is_some_and(|candidate| candidate.bvid == part.bvid)
                })
                .cloned()
                .collect::<Vec<_>>();
            menu = menu.item(
                PopupMenuItem::new(t!("Play all parts of this video"))
                    .disabled(!playback_allowed)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !playlist_target.is_none_or(|target| target.can_play(&this.state)) {
                            return;
                        }
                        this.replace_queue_and_play(parts.clone(), 0, QueueOrigin::Playlist);
                        cx.notify();
                    })),
            );
        }
        if is_bilibili && browsing_owner && capabilities.video_favorite_write {
            if let (Some(owner), Some(part)) = (song.source.clone(), song.bilibili_part.as_ref()) {
                let bvid = part.bvid.clone();
                menu = menu
                    .separator()
                    .item(
                        PopupMenuItem::new(t!("Save video to…")).on_click(cx.listener(
                            move |this, _, window, cx| {
                                if !playlist_target
                                    .is_none_or(|target| target.is_current(&this.state))
                                {
                                    return;
                                }
                                this.open_bilibili_favorite(
                                owner.clone(),
                                bvid.clone(),
                                crate::app::bilibili::favorite_state::FavoriteDialogKind::Picker,
                                window,
                                cx,
                            );
                            },
                        )),
                    );
            }
        }
        if !is_bilibili {
            let radio_seed = song.clone();
            let radio_loading = self.song_radio_loading(&song.id);
            menu = menu.item(
                PopupMenuItem::new(if radio_loading {
                    t!("Building song radio…")
                } else {
                    t!("Start song radio")
                })
                .disabled(
                    self.source_for_song(&song)
                        .is_none_or(|api| !api.capabilities().recommendations)
                        || radio_loading,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.start_song_radio(radio_seed.clone());
                    cx.notify();
                })),
            );
            let add = song.clone();
            menu = menu.separator().item(
                PopupMenuItem::new(t!("Add to playlist…"))
                    .disabled(
                        self.state.playlists.editing()
                            || !browsing_owner
                            || !capabilities.playlist_write,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.browsing_owns_song(&add) {
                            this.choose_playlist(vec![add.id.clone()], window, cx)
                        }
                    })),
            );
            let key = FavoriteKey::new(FavoriteKind::Song, &song.id);
            let label = if self.state.favorites.is_starred(&key) {
                t!("Remove from favorites")
            } else {
                t!("Add to favorites")
            };
            let favorite_song = song.clone();
            menu = menu.item(
                PopupMenuItem::new(label)
                    .disabled(
                        self.state.favorites.is_pending(&key)
                            || !browsing_owner
                            || !capabilities.favorite_write,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.browsing_owns_song(&favorite_song) {
                            this.toggle_favorite(FavoriteKey::new(
                                FavoriteKind::Song,
                                &favorite_song.id,
                            ));
                        }
                        cx.notify();
                    })),
            );
            // 与行内可点击的艺术家/专辑名一致：菜单也提供导航入口，
            // 缺少服务器 id（如整理过的杂项曲库）时禁用。
            let song_for_artist = song.clone();
            let artist_available =
                browsing_owner && capabilities.library && artist_for_song(&song).is_some();
            menu = menu.item(
                PopupMenuItem::new(t!("Go to artist"))
                    .disabled(!artist_available)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_song_artist(&song_for_artist);
                        cx.notify();
                    })),
            );
            let song_for_album = song.clone();
            let album_available =
                browsing_owner && capabilities.library && album_for_song(&song).is_some();
            menu = menu.item(
                PopupMenuItem::new(t!("Go to album"))
                    .disabled(!album_available)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_song_album(&song_for_album);
                        cx.notify();
                    })),
            );
        }
        let is_local = song
            .source
            .as_ref()
            .is_some_and(|source| source.provider == crate::source::ProviderKind::LocalFolder);
        if !is_local && browsing_owner {
            let offline_status = self.song_offline_status(&song);
            let offline_song = song.clone();
            let offline_label = match offline_status {
                crate::offline::OfflineTrackStatus::Ready => t!("Remove offline copy"),
                crate::offline::OfflineTrackStatus::Downloading => t!("Downloading offline…"),
                crate::offline::OfflineTrackStatus::NotPinned => t!("Download for offline"),
            };
            menu = menu.item(
                PopupMenuItem::new(offline_label)
                    .disabled(
                        offline_status == crate::offline::OfflineTrackStatus::Downloading
                            || (self.offline_mode_active()
                                && offline_status
                                    == crate::offline::OfflineTrackStatus::NotPinned),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match offline_status {
                            crate::offline::OfflineTrackStatus::Ready => {
                                this.unpin_song_offline(&offline_song);
                            }
                            crate::offline::OfflineTrackStatus::NotPinned => {
                                this.pin_song_offline(offline_song.clone());
                            }
                            crate::offline::OfflineTrackStatus::Downloading => {}
                        }
                        cx.notify();
                    })),
            );
        }
        let song_for_info = song.clone();
        menu = menu
            .separator()
            .item(PopupMenuItem::new(t!("Song info…")).on_click(cx.listener(
                move |this, _, window, cx| {
                    if !playlist_target.is_none_or(|target| target.is_current(&this.state)) {
                        return;
                    }
                    this.open_song_info(song_for_info.clone(), window, cx);
                },
            )));
        if let SongMenuSource::Library {
            songs,
            index,
            playlist: Some(playlist),
            ..
        } = context.source
        {
            if is_bilibili {
                if let (Some(owner), Some(part), Ok(folder_id)) = (
                    song.source.clone(),
                    song.bilibili_part.as_ref(),
                    playlist.id.parse::<u64>(),
                ) {
                    let bvid = part.bvid.clone();
                    let name = playlist.name.clone();
                    menu = menu.separator().item(PopupMenuItem::new(t!("Remove from this favorite folder"))
                        .disabled(!capabilities.video_favorite_write || !browsing_owner || !playback_allowed || self.favorite_ui.writing)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if !playlist_target.is_none_or(|target| target.can_play(&this.state)) { return; }
                            this.open_bilibili_favorite(owner.clone(), bvid.clone(),
                                crate::app::bilibili::favorite_state::FavoriteDialogKind::Remove { folder_id, folder_name: name.clone() }, window, cx);
                        })));
                }
                return menu;
            }
            let disabled = self.state.playlists.editing()
                || self.state.playlists.detail_loading()
                || !playback_allowed
                || !can_edit(&playlist, &self.config.username);
            let expected = song_ids(&songs);
            menu = menu.separator();
            for (label, to) in [
                (t!("Move up"), index.checked_sub(1)),
                (
                    t!("Move down"),
                    (index + 1 < expected.len()).then_some(index + 1),
                ),
            ] {
                let edit = PlaylistEdit::Move {
                    id: playlist.id.clone(),
                    expected: expected.clone(),
                    from: index,
                    to: to.unwrap_or(index),
                };
                menu = menu.item(
                    PopupMenuItem::new(label)
                        .disabled(
                            disabled
                                || to.is_none()
                                || playlist_target.is_some_and(|target| target.filtered),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !playlist_target.is_none_or(|target| target.can_play(&this.state))
                                || this.state.playlists.search.filtered()
                            {
                                return;
                            }
                            this.start_playlist_edit(edit.clone(), PlaylistEditOrigin::Menu, cx)
                        })),
                );
            }
            let edit = PlaylistEdit::Remove {
                id: playlist.id,
                expected,
                index,
            };
            let message = t!(
                "Remove “%{title}” (track %{number}) from “%{playlist}”?",
                title = song.title,
                number = index + 1,
                playlist = playlist.name
            )
            .to_string();
            menu = menu.item(
                PopupMenuItem::new(t!("Remove from playlist…"))
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if !playlist_target.is_none_or(|target| target.can_play(&this.state)) {
                            return;
                        }
                        this.confirm_playlist_edit(
                            edit.clone(),
                            t!("Remove track?").to_string(),
                            message.clone(),
                            window,
                            cx,
                        )
                    })),
            );
        } else if let SongMenuSource::Queue(entry) = context.source {
            let current_index = queue_menu_index(&self.state.queue, entry);
            let queue_len = self.state.queue.len();
            let move_next_target = current_index.and_then(|index| {
                crate::app::queue::undo::queue_move_to_next_target(
                    index,
                    self.state.queue_index,
                    queue_len,
                )
            });
            let can_move_up = current_index.is_some_and(|index| index > 0);
            let can_move_down = current_index.is_some_and(|index| index + 1 < queue_len);
            menu =
                menu.separator()
                    .item(
                        PopupMenuItem::new(t!("Move to next"))
                            .disabled(move_next_target.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(index) = queue_menu_index(&this.state.queue, entry) {
                                    if let Some(to) =
                                        crate::app::queue::undo::queue_move_to_next_target(
                                            index,
                                            this.state.queue_index,
                                            this.state.queue.len(),
                                        )
                                    {
                                        this.move_queue_item(index, to);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .item(
                        PopupMenuItem::new(t!("Move up"))
                            .disabled(!can_move_up)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(index) = queue_menu_index(&this.state.queue, entry) {
                                    if let Some(to) = index.checked_sub(1) {
                                        this.move_queue_item(index, to);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .item(
                        PopupMenuItem::new(t!("Move down"))
                            .disabled(!can_move_down)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(index) = queue_menu_index(&this.state.queue, entry) {
                                    if index + 1 < this.state.queue.len() {
                                        this.move_queue_item(index, index + 1);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .item(
                        PopupMenuItem::new(t!("Move to end"))
                            .disabled(!can_move_down)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(index) = queue_menu_index(&this.state.queue, entry) {
                                    let last = this.state.queue.len().saturating_sub(1);
                                    if index < last {
                                        this.move_queue_item(index, last);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new(t!("Remove from queue")).on_click(cx.listener(
                            move |this, _, _, cx| {
                                if let Some(index) = queue_menu_index(&this.state.queue, entry) {
                                    this.remove_from_queue(index);
                                }
                                cx.notify();
                            },
                        )),
                    );
        }
        menu
    }
}

#[cfg(test)]
mod tests;
