//! User-confirmed favorite actions. Accepted writes outlive dialog/source changes.

use super::favorite_state::FavoriteDialogKind;
use crate::app::prelude::*;
use crate::source::{
    bilibili::favorites::{FavoriteOperation, FavoriteOutcome, FavoriteSnapshot},
    ProviderKind, SourceId,
};
use std::sync::atomic::AtomicUsize;

struct PendingWrite(Arc<AtomicUsize>);
impl PendingWrite {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}
impl Drop for PendingWrite {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl NavidromeApp {
    pub(in crate::app) fn open_bilibili_favorite(
        &mut self,
        source: SourceId,
        bvid: String,
        kind: FavoriteDialogKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.favorite_ui.open
            || self.favorite_ui.writing
            || self.state.settings.dialog.is_some()
            || self.playlist_ui.dialog_open()
            || window.has_active_dialog(cx)
            || self.api.as_ref().map(|api| api.id()) != Some(&source)
        {
            return;
        }
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.capabilities().video_favorite_write)
        else {
            return;
        };
        let request = self.library_requests.next();
        let label = self.active_server_label();
        if !self
            .favorite_ui
            .begin(source.clone(), label, bvid.clone(), kind, request)
        {
            return;
        }
        self.spawn_favorite_read(api, source, bvid, request);
        let app = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let closing = app.clone();
            window.open_dialog(cx, move |dialog, _, cx| {
                let closing = closing.clone();
                app.update(cx, |this, cx| {
                    let dialog = this.render_bilibili_favorite_dialog(dialog, cx).on_close(
                        move |_, _, cx| {
                            let _ = closing.update(cx, |this, cx| {
                                this.favorite_ui.close();
                                cx.notify();
                            });
                        },
                    );
                    crate::app::style::surface::raised_dialog(dialog, cx)
                })
                .expect("favorite dialog belongs to app")
            });
        });
        cx.notify();
    }

    fn spawn_favorite_read(
        &mut self,
        api: MusicSource,
        source: SourceId,
        bvid: String,
        request: LibraryRequest,
    ) {
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result = tokio::time::timeout(
                Duration::from_secs(30),
                api.prepare_bilibili_favorite(&bvid),
            )
            .await
            .map_err(|_| t!("Favorite status lookup timed out. Retry.").to_string())
            .and_then(|result| result.map_err(error_message));
            let _ = tx.send(Msg::BilibiliFavoriteRead {
                source,
                request,
                result,
            });
        });
        self.favorite_ui.attach_read(task.abort_handle());
    }

    pub(in crate::app) fn reload_bilibili_favorite(&mut self, cx: &mut Context<Self>) {
        if self.favorite_ui.writing {
            return;
        }
        let Some(source) = self.favorite_ui.source.clone() else {
            return;
        };
        let Some(api) = self.api.clone().filter(|api| api.id() == &source) else {
            self.favorite_ui.error = Some(
                t!("The music source changed. Close this dialog and open it again.").to_string(),
            );
            cx.notify();
            return;
        };
        let request = self.library_requests.next();
        self.favorite_ui.begin_read(request);
        self.spawn_favorite_read(api, source, self.favorite_ui.bvid.clone(), request);
        cx.notify();
    }

    pub(in crate::app) fn submit_bilibili_favorite(
        &mut self,
        operation: FavoriteOperation,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.favorite_ui.source.clone() else {
            return;
        };
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.id() == &source && api.id().provider == ProviderKind::Bilibili)
        else {
            self.favorite_ui.error = Some(
                t!("The music source changed. Close this dialog and open it again.").to_string(),
            );
            cx.notify();
            return;
        };
        let request = self.library_requests.next();
        let Some(snapshot) = self.favorite_ui.begin_write(request, operation) else {
            return;
        };
        let pending = PendingWrite::new(self.favorite_writes.clone());
        let tx = self.tx.clone();
        // No abort handle or outer timeout: once a write is accepted we let its
        // bounded POST and verification finish, even if the window is leaving.
        self.runtime.spawn(async move {
            let _pending = pending;
            let result = api
                .change_bilibili_favorite(&snapshot, operation)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::BilibiliFavoriteWrite {
                source,
                request,
                result,
            });
        });
        cx.notify();
    }

    pub(in crate::app) fn finish_bilibili_favorite_read(
        &mut self,
        source: SourceId,
        request: LibraryRequest,
        result: Result<FavoriteSnapshot, String>,
    ) {
        let result = if self.api.as_ref().map(|api| api.id()) == Some(&source) {
            result
        } else {
            Err(t!("The music source changed. Close this dialog and open it again.").to_string())
        };
        if self.favorite_ui.complete_read(&source, request, result) {
            self.refresh_bilibili_favorite_views(&source);
        }
    }

    fn refresh_bilibili_favorite_views(&mut self, source: &SourceId) {
        if self.favorite_ui.operation.is_none()
            || self.favorite_ui.uncertain
            || self.favorite_ui.error.is_some()
            || self.api.as_ref().map(|api| api.id()) != Some(source)
        {
            return;
        }
        let folder_id = self
            .favorite_ui
            .operation
            .map(|operation| operation.folder_id.to_string());
        self.load_playlists();
        if self.state.view == View::PlaylistDetail
            && self
                .state
                .playlists
                .playlist()
                .is_some_and(|folder| Some(&folder.id) == folder_id.as_ref())
        {
            self.load_playlist_detail();
        }
    }

    pub(in crate::app) fn finish_bilibili_favorite_write(
        &mut self,
        source: SourceId,
        request: LibraryRequest,
        result: Result<FavoriteOutcome, String>,
    ) {
        if !self.favorite_ui.complete_write(&source, request, result) {
            return;
        }
        self.refresh_bilibili_favorite_views(&source);
    }

    pub(in crate::app) fn close_bilibili_favorite(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.favorite_ui.close() {
            window.close_dialog(cx);
            cx.notify();
        }
    }
}
