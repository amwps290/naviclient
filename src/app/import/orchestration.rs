//! Playlist import orchestration and input/focus ownership.

use super::flow::ImportFlow;
use crate::app::prelude::*;
use crate::import;
use crate::playlist::{PlaylistEdit, PlaylistEdited};

#[derive(Default)]
pub(in crate::app) struct ImportUi {
    pub(in crate::app) flow: ImportFlow,
    pub(in crate::app) url_input: Option<Entity<InputState>>,
    pub(in crate::app) name_input: Option<Entity<InputState>>,
    subscription: Option<Subscription>,
}

impl ImportUi {
    pub(in crate::app) fn reset(&mut self) {
        // Dropping the old flow aborts its active task and invalidates queued results.
        *self = Self::default();
    }

    /// Saving owns the review until success; failures keep its matched entries.
    pub(in crate::app) fn complete_playlist_edit(
        &mut self,
        result: &Result<PlaylistEdited, String>,
    ) -> bool {
        match result {
            Ok(_) => {
                let open = self.flow.stage().is_some();
                self.reset();
                open
            }
            Err(error) => {
                if self.flow.stage() == Some(ImportStage::Review) {
                    self.flow.set_error(error.clone());
                }
                false
            }
        }
    }
}

impl NavidromeApp {
    pub(in crate::app) fn import_busy(&self) -> bool {
        self.import_ui.flow.stage() == Some(ImportStage::Matching) || self.state.playlists.editing()
    }

    pub(in crate::app) fn open_import_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.api.is_none()
            || self.state.playlists.editing()
            || self.playlist_ui.dialog_open()
            || self.import_ui.flow.stage().is_some()
        {
            return;
        }
        self.import_ui.reset();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(t!("Playlist link")));
        self.import_ui.subscription =
            Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_import_step(cx);
                }
            }));
        self.import_ui.url_input = Some(input.clone());
        self.import_ui.flow.open();
        self.show_import_dialog(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn show_import_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            // Wait for menu focus restoration before installing the dialog scope.
            window.defer(cx, move |window, cx| {
                let render_app = app.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| this.render_import_dialog(dialog, cx))
                        .expect("app owns dialog")
                });
                let _ = app.update(cx, |this, cx| {
                    if this.import_ui.flow.stage() == Some(ImportStage::Url) {
                        if let Some(input) = this.import_ui.url_input.clone() {
                            input.update(cx, |input, cx| input.focus(window, cx));
                        }
                    }
                });
            });
        }
        cx.notify();
    }

    pub(in crate::app) fn submit_import_step(&mut self, cx: &mut Context<Self>) {
        match self.import_ui.flow.stage() {
            Some(ImportStage::Url) => self.submit_import_url(cx),
            Some(ImportStage::Review) => self.submit_import_playlist(cx),
            Some(ImportStage::Matching) | None => {}
        }
    }

    fn submit_import_url(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.import_ui.url_input.clone() else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.import_ui
                .flow
                .set_error(t!("Unrecognized playlist link.").to_string());
            cx.notify();
            return;
        }
        let Some(request) = self
            .import_ui
            .flow
            .begin_resolve(&mut self.library_requests)
        else {
            return;
        };
        let http = import::http_client();
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result = async {
                let source = import::resolve(&text, &http).await?;
                source.fetch(&http).await
            }
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::ImportResolved { request, result });
        });
        self.import_ui.flow.set_task(request, task.abort_handle());
        cx.notify();
    }

    pub(in crate::app) fn finish_import_resolve(
        &mut self,
        request: LibraryRequest,
        result: Result<import::ImportedPlaylist, String>,
        cx: &mut Context<Self>,
    ) {
        let api = self.api.clone();
        let result = if api.is_none() {
            result.and_then(|_| Err(t!("Not connected").to_string()))
        } else {
            result
        };
        let Some(result) =
            self.import_ui
                .flow
                .complete_resolve(request, result, &mut self.library_requests)
        else {
            return;
        };
        if let (Ok(request), Some(api)) = (result, api) {
            let tracks = self
                .import_ui
                .flow
                .matching_playlist()
                .expect("accepted resolution has source tracks")
                .tracks
                .clone();
            let tx = self.tx.clone();
            let task = self.runtime.spawn(async move {
                let progress = {
                    let tx = tx.clone();
                    move |done: usize| {
                        let _ = tx.send(Msg::ImportProgress { request, done });
                    }
                };
                let result = import::match_playlist(&api, &tracks, progress)
                    .await
                    .map_err(error_message);
                let _ = tx.send(Msg::ImportMatched { request, result });
            });
            self.import_ui.flow.set_task(request, task.abort_handle());
        }
        cx.notify();
    }

    pub(in crate::app) fn finish_import_progress(&mut self, request: LibraryRequest, done: usize) {
        self.import_ui.flow.update_progress(request, done);
    }

    pub(in crate::app) fn finish_import_match(
        &mut self,
        request: LibraryRequest,
        result: Result<import::MatchedSongs, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.import_ui.flow.complete_match(request, result) {
            return;
        }
        if let Some(review) = self.import_ui.flow.review() {
            let name = review.name.clone();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("Playlist name"))
                    .default_value(name)
            });
            self.import_ui.subscription =
                Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit_import_step(cx);
                    }
                }));
            self.import_ui.name_input = Some(input.clone());
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    fn submit_import_playlist(&mut self, cx: &mut Context<Self>) {
        if self.state.playlists.editing() {
            return;
        }
        let Some(input) = self.import_ui.name_input.clone() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.import_ui
                .flow
                .set_error(t!("Enter a playlist name.").to_string());
            cx.notify();
            return;
        }
        let Some(review) = self.import_ui.flow.review() else {
            return;
        };
        let songs = review.song_ids();
        if songs.is_empty() {
            self.import_ui
                .flow
                .set_error(t!("No tracks matched your library.").to_string());
            cx.notify();
            return;
        }
        self.start_playlist_edit(
            PlaylistEdit::Create { name, songs },
            PlaylistEditOrigin::Import,
            cx,
        );
    }

    pub(in crate::app) fn close_import_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A submitted createPlaylist cannot reliably be undone. Keep the dialog
        // protected until saving finishes; parsing and matching can be cancelled.
        if self.state.playlists.editing() {
            return;
        }
        self.import_ui.reset();
        window.close_dialog(cx);
        cx.notify();
    }
}
