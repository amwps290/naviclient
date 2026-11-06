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
    },
}

impl SongMenuContext {
    fn for_scope(state: &AppState, scope: SongScope, index: usize) -> Option<Self> {
        let songs = state.songs_for_scope(scope)?;
        let song = songs.get(index)?.clone();
        let source = if scope == SongScope::Queue {
            SongMenuSource::Queue(state.queue.entry_id(index)?)
        } else {
            SongMenuSource::Library {
                songs: Arc::new(songs.to_vec()),
                index,
                playlist: (scope == SongScope::PlaylistDetail)
                    .then(|| state.playlists.playlist().cloned())
                    .flatten(),
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
        let play = context.source.clone();
        menu = menu.item(PopupMenuItem::new(t!("Play now")).on_click(cx.listener(
            move |this, _, _, cx| {
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
            },
        )));
        let next = song.clone();
        menu = menu.item(PopupMenuItem::new(t!("Play next")).on_click(cx.listener(
            move |this, _, _, cx| {
                this.insert_next(next.clone());
                cx.notify();
            },
        )));
        let append = song.clone();
        menu = menu.item(PopupMenuItem::new(t!("Add to queue")).on_click(cx.listener(
            move |this, _, _, cx| {
                this.add_to_queue(append.clone());
                cx.notify();
            },
        )));
        let radio_seed = song.clone();
        let radio_loading = self.song_radio_loading(&song.id);
        menu = menu.item(
            PopupMenuItem::new(if radio_loading {
                t!("Building song radio…")
            } else {
                t!("Start song radio")
            })
            .disabled(self.api.is_none() || radio_loading)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.start_song_radio(radio_seed.clone());
                cx.notify();
            })),
        );
        let add = song.id.clone();
        menu = menu.separator().item(
            PopupMenuItem::new(t!("Add to playlist…"))
                .disabled(self.state.playlists.editing() || self.api.is_none())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose_playlist(vec![add.clone()], window, cx)
                })),
        );
        let key = FavoriteKey::new(FavoriteKind::Song, &song.id);
        let label = if self.state.favorites.is_starred(&key) {
            t!("Remove from favorites")
        } else {
            t!("Add to favorites")
        };
        let favorite_id = song.id.clone();
        menu = menu.item(
            PopupMenuItem::new(label)
                .disabled(self.state.favorites.is_pending(&key))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_favorite(FavoriteKey::new(FavoriteKind::Song, &favorite_id));
                    cx.notify();
                })),
        );
        // 与行内可点击的艺术家/专辑名一致：菜单也提供导航入口，
        // 缺少服务器 id（如整理过的杂项曲库）时禁用。
        let song_for_artist = song.clone();
        let artist_available = artist_for_song(&song).is_some();
        menu = menu.item(
            PopupMenuItem::new(t!("Go to artist"))
                .disabled(!artist_available)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_song_artist(&song_for_artist);
                    cx.notify();
                })),
        );
        let song_for_album = song.clone();
        let album_available = album_for_song(&song).is_some();
        menu = menu.item(
            PopupMenuItem::new(t!("Go to album"))
                .disabled(!album_available)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_song_album(&song_for_album);
                    cx.notify();
                })),
        );
        let song_for_info = song.clone();
        menu = menu
            .separator()
            .item(PopupMenuItem::new(t!("Song info…")).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.open_song_info(song_for_info.clone(), window, cx);
                },
            )));
        if let SongMenuSource::Library {
            songs,
            index,
            playlist: Some(playlist),
        } = context.source
        {
            let disabled = self.state.playlists.editing()
                || self.state.playlists.detail_loading()
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
                        .disabled(disabled || to.is_none())
                        .on_click(cx.listener(move |this, _, _, cx| {
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
            menu = menu
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
