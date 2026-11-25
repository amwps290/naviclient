//! Bounded index construction during streaming, local filtering and input lifecycle.

use super::search_index::{CancelOnDrop, Cancellation, PlaylistSearchIndex};
use super::search_state::{PlaylistSearchRequest, SearchWork};
use crate::app::prelude::*;
use crate::playlist::loading::{playlist_update_channel, PlaylistLoadError, PlaylistUpdate};

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(100);

/// Both provider channels are bounded. Each batch is indexed on a blocking
/// worker before ownership passes to the UI, preserving previews and order.
pub(in crate::app) async fn load_indexed_playlist(
    api: MusicSource,
    playlist_id: String,
    request: LibraryRequest,
    updates: tokio::sync::mpsc::Sender<PlaylistUpdate>,
    tx: Sender<Msg>,
) {
    let (provider_tx, mut provider_rx) = playlist_update_channel();
    let cancel = Cancellation::default();
    let _guard = CancelOnDrop(cancel.clone());
    let forward = async move {
        let mut index = PlaylistSearchIndex::default();
        while let Some(update) = provider_rx.recv().await {
            let update = match update {
                PlaylistUpdate::Batch { songs, progress } => {
                    let cancellation = cancel.clone();
                    let built = tokio::task::spawn_blocking(move || {
                        index.append(&songs, &cancellation).map(|()| (index, songs))
                    })
                    .await;
                    match built {
                        Ok(Some((built_index, songs))) => {
                            index = built_index;
                            PlaylistUpdate::Batch { songs, progress }
                        }
                        Ok(None) => return,
                        Err(_) => {
                            let _ = updates
                                .send(PlaylistUpdate::Failed(PlaylistLoadError::interrupted()))
                                .await;
                            return;
                        }
                    }
                }
                PlaylistUpdate::Complete(progress) => {
                    // Publish before Complete: stream completion may abort this
                    // task. UI accepts the index on either side of completion.
                    let _ = tx.send(Msg::PlaylistIndexed {
                        request,
                        index: Arc::new(index),
                    });
                    let _ = updates.send(PlaylistUpdate::Complete(progress)).await;
                    return;
                }
                update => update,
            };
            let failed = matches!(update, PlaylistUpdate::Failed(_));
            if updates.send(update).await.is_err() || failed {
                return;
            }
        }
    };
    tokio::join!(api.load_playlist(&playlist_id, provider_tx), forward);
}

impl AppState {
    pub(in crate::app) fn set_playlist_query(&mut self, query: String) -> bool {
        if !self.playlists.search.set_query(query) {
            return false;
        }
        self.clear_song_selection_for_scope(SongScope::PlaylistDetail);
        true
    }
}

impl NavidromeApp {
    pub(in crate::app) fn subscribe_playlist_search(&mut self, cx: &mut Context<Self>) {
        let input = self.playlist_search_input.clone();
        self._subscriptions
            .push(cx.subscribe(&input, |this, input, event, cx| {
                if matches!(event, InputEvent::Change) && this.state.view == View::PlaylistDetail {
                    this.set_playlist_search_query(input.read(cx).value().to_string(), cx);
                } else if matches!(event, InputEvent::Focus | InputEvent::Blur) {
                    // The capsule draws a parent-owned focus border. Update
                    // its color for mouse focus as well as Ctrl+F.
                    cx.notify();
                }
            }));
    }

    pub(in crate::app) fn set_playlist_search_query(
        &mut self,
        query: String,
        cx: &mut Context<Self>,
    ) {
        let was_filtered = self.state.playlists.search.filtered();
        let scroll = self.song_list_scroll(SongScope::PlaylistDetail);
        let before = scroll.0.borrow().base_handle.logical_scroll_top().0;
        if !self.state.set_playlist_query(query) {
            return;
        }
        let filtered = self.state.playlists.search.filtered();
        if !was_filtered && filtered {
            self.state.playlists.search.before_search_row = Some(before);
            self.state.playlists.search.located = None;
        }
        if !filtered {
            let search = &mut self.state.playlists.search;
            let located = search.located.take();
            let target = located.or(search.before_search_row.take());
            if let Some(target) = target.filter(|_| !self.state.playlists.songs().is_empty()) {
                let len = self.state.playlists.songs().len();
                let target = target.min(len - 1);
                scroll.scroll_to_item_strict(target, gpui_kit::ScrollStrategy::Top);
                if located.is_some() {
                    self.state
                        .song_selection
                        .select(SongScope::PlaylistDetail, target, len);
                }
            }
            self.load_visible_song_covers(SongScope::PlaylistDetail);
        }
        cx.notify();
    }

    pub(in crate::app) fn toggle_playlist_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.playlists.search.expanded {
            self.close_playlist_search(window, cx);
        } else {
            self.focus_playlist_search(window, cx);
        }
    }

    pub(in crate::app) fn close_playlist_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.playlist_search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.set_playlist_search_query(String::new(), cx);
        self.state.playlists.search.expanded = false;
        self.focus_playlist_list(window, cx);
    }

    pub(in crate::app) fn focus_playlist_list(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.playlist_list_focus.focus(window, cx);
        if self.state.song_scope_ready(SongScope::PlaylistDetail)
            && self.state.song_selection.cursor().is_none()
            && self
                .state
                .songs_for_scope(SongScope::PlaylistDetail)
                .is_some_and(|songs| !songs.is_empty())
        {
            self.select_song_row(SongScope::PlaylistDetail, 0);
        }
        cx.notify();
    }

    pub(in crate::app) fn focus_playlist_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.playlists.search.expanded = true;
        self.state.queue_drawer_open = false;
        self.playlist_search_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(in crate::app) fn poll_playlist_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Page/source resets own the query; programmatic set_value emits no Change.
        let query = self.state.playlists.search.query.clone();
        let placeholder = if self.browsing_bilibili() {
            t!("Search songs, uploaders, videos or parts…")
        } else {
            t!("Search song titles, artists or albums…")
        };
        let input = self.playlist_search_input.read(cx);
        let value_changed = input.value().as_ref() != query;
        let placeholder_changed =
            input.presentation().placeholder().as_ref() != placeholder.as_ref();
        if value_changed || placeholder_changed {
            self.playlist_search_input.update(cx, |input, cx| {
                if value_changed {
                    input.set_value(query, window, cx);
                }
                if placeholder_changed {
                    input.set_placeholder(placeholder.to_string(), window, cx);
                }
            });
        }
        if !self.state.playlists.detail_ready() {
            return value_changed || placeholder_changed;
        }
        let Some((request, index, terms)) = self.state.playlists.search.start() else {
            return value_changed || placeholder_changed;
        };
        let cancel = Cancellation::default();
        let cancellation = cancel.clone();
        let guard = CancelOnDrop(cancel.clone());
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let _guard = guard;
            tokio::time::sleep(SEARCH_DEBOUNCE).await;
            let result =
                tokio::task::spawn_blocking(move || index.matching(&terms, &cancellation)).await;
            let result = match result {
                Ok(Some(indices)) => Ok(indices),
                Ok(None) => return,
                Err(_) => Err("Playlist search worker stopped".into()),
            };
            let _ = tx.send(Msg::PlaylistFiltered { request, result });
        });
        self.state.playlists.search.attach(SearchWork {
            abort: task.abort_handle(),
            cancel,
        });
        true
    }

    pub(in crate::app) fn finish_playlist_search(
        &mut self,
        request: PlaylistSearchRequest,
        result: Result<Vec<usize>, String>,
    ) {
        if self.state.playlists.snapshot() != Some(request.snapshot)
            || !self.state.playlists.detail_ready()
            || !self.state.playlists.search.complete(request, result)
        {
            return;
        }
        self.state
            .clear_song_selection_for_scope(SongScope::PlaylistDetail);
        self.song_list_scroll(SongScope::PlaylistDetail)
            .scroll_to_item_strict(0, gpui_kit::ScrollStrategy::Top);
        self.load_visible_song_covers(SongScope::PlaylistDetail);
    }
}

#[cfg(test)]
mod tests;
