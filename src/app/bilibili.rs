//! Bilibili's local home and folder browser. Network discovery is a later batch.

mod discovery;
pub(in crate::app) mod discovery_state;
mod discovery_view;
pub(in crate::app) mod state;
mod view;

use crate::app::prelude::*;
use crate::models::ListeningHistory;
use crate::source::SourceId;

impl NavidromeApp {
    pub(in crate::app) fn subscribe_bilibili_folder_search(&mut self, cx: &mut Context<Self>) {
        let input = self.bilibili_folder_search.clone();
        self._subscriptions
            .push(cx.subscribe(&input, |this, input, event, cx| {
                if matches!(event, InputEvent::Change)
                    && this.browsing_bilibili()
                    && this.state.view == View::Playlists
                {
                    this.state
                        .bilibili
                        .set_folder_query(input.read(cx).value().to_string());
                    cx.notify();
                } else if matches!(event, InputEvent::Focus | InputEvent::Blur) {
                    cx.notify();
                }
            }));
    }

    pub(in crate::app) fn load_bilibili_home(&mut self) {
        let Some(source) = self.api.as_ref().map(|api| api.id().clone()) else {
            return;
        };
        self.state.bilibili_discovery.bind(&source);
        let pending = self.server_cache_writer.pending_source_history(&source);
        let request = self
            .state
            .bilibili
            .begin_history(source.clone(), &mut self.library_requests);
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let read_source = source.clone();
            let result = tokio::task::spawn_blocking(move || {
                pending.unwrap_or_else(|| config::load_source_history(&read_source))
            })
            .await
            .map_err(|_| "history reader stopped".to_string());
            let _ = tx.send(Msg::BilibiliHistory {
                source,
                request,
                result,
            });
        });
        self.state.bilibili.attach_history(task.abort_handle());
    }

    pub(in crate::app) fn finish_bilibili_home(
        &mut self,
        source: SourceId,
        request: LibraryRequest,
        result: Result<ListeningHistory, String>,
    ) {
        if self.api.as_ref().map(|api| api.id()) != Some(&source)
            || !self
                .state
                .bilibili
                .complete_history(&source, request, result)
        {
            return;
        }
        let covers = self
            .bilibili_recent_tracks()
            .iter()
            .filter_map(|song| song.cover_art.clone())
            .collect();
        self.preload_covers(covers);
    }

    pub(in crate::app) fn bilibili_recent_tracks(&self) -> Vec<&Song> {
        let Some(source) = self.api.as_ref().map(|api| api.id()) else {
            return Vec::new();
        };
        let history = if self.playback_source.as_ref().map(|api| api.id()) == Some(source) {
            self.state.listening.history()
        } else {
            &self.state.bilibili.history
        };
        state::recent_tracks(history, source)
    }

    pub(in crate::app) fn poll_bilibili_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let query = &self.state.bilibili.folder_query;
        let placeholder = t!("Find a favorite folder…");
        let input = self.bilibili_folder_search.read(cx);
        let value_changed = input.value().as_ref() != query;
        let placeholder_changed =
            input.presentation().placeholder().as_ref() != placeholder.as_ref();
        let changed = value_changed || placeholder_changed;
        if changed {
            let query = query.clone();
            self.bilibili_folder_search.update(cx, |input, cx| {
                if value_changed {
                    input.set_value(query, window, cx);
                }
                if placeholder_changed {
                    input.set_placeholder(placeholder.to_string(), window, cx);
                }
            });
        }
        let draft = self.state.bilibili_discovery.draft.clone();
        let search_placeholder = t!("Search Bilibili videos or paste a BV/AV ID or link…");
        let search_input = self.bilibili_search_input.read(cx);
        let search_value_changed = search_input.value().as_ref() != draft;
        let search_placeholder_changed =
            search_input.presentation().placeholder().as_ref() != search_placeholder.as_ref();
        if search_value_changed || search_placeholder_changed {
            self.bilibili_search_input.update(cx, |input, cx| {
                if search_value_changed {
                    input.set_value(draft, window, cx);
                }
                if search_placeholder_changed {
                    input.set_placeholder(search_placeholder.to_string(), window, cx);
                }
            });
        }
        changed || search_value_changed || search_placeholder_changed
    }
}
