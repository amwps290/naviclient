//! Search/preview state is independent of currently displayed lyrics.

use crate::app::prelude::*;
use crate::lyrics::{
    matching,
    online::Provider,
    provider::{ProviderError, ProviderId, Query, Record, SearchOutcome},
    service::{LyricsService, LyricsStatus},
    Document,
};
use crate::source::MediaKey;
use gpui_kit::component::dialog::Dialog;

fn lyrics_dialog_behavior(dialog: Dialog) -> Dialog {
    dialog
        .keyboard(true)
        .overlay_closable(false)
        // InputEvent::PressEnter owns search/offset application. The dialog's
        // bubbled Confirm must neither repeat that action nor cancel the task.
        .on_ok(|_, _, _| false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum LyricsDialog {
    Search,
    Timing,
}

pub(in crate::app) struct LyricsUi {
    pub title: Entity<InputState>,
    pub artist: Entity<InputState>,
    pub album: Entity<InputState>,
    pub offset: Entity<InputState>,
    pub dialog: Option<LyricsDialog>,
    pub media: Option<MediaKey>,
    pub song_title: String,
    pub query: Query,
    pub records: Vec<Record>,
    pub selected: Option<usize>,
    pub request: Option<LibraryRequest>,
    pub error: Option<LyricsStatus>,
    pub source: Option<ProviderId>,
    pub failures: Vec<(ProviderId, LyricsStatus)>,
    pub remember: bool,
    pub plain: bool,
    pub searched: bool,
    pub candidates_scroll: ScrollHandle,
    task: Option<tokio::task::JoinHandle<()>>,
    close_requested: bool,
}

impl LyricsUi {
    pub fn new<T: 'static>(window: &mut Window, cx: &mut Context<T>) -> Self {
        Self {
            title: cx.new(|cx| InputState::new(window, cx)),
            artist: cx.new(|cx| InputState::new(window, cx)),
            album: cx.new(|cx| InputState::new(window, cx)),
            offset: cx.new(|cx| InputState::new(window, cx)),
            dialog: None,
            media: None,
            song_title: String::new(),
            query: Default::default(),
            records: Vec::new(),
            selected: None,
            request: None,
            error: None,
            source: None,
            failures: Vec::new(),
            remember: true,
            plain: false,
            searched: false,
            candidates_scroll: ScrollHandle::new(),
            task: None,
            close_requested: false,
        }
    }
    pub fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.request = None;
        self.media = None;
        if self.dialog.take().is_some() {
            self.close_requested = true;
        }
    }
}
impl Drop for LyricsUi {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl NavidromeApp {
    pub(in crate::app) fn lyrics_service(&mut self) -> LyricsService {
        let sources = self.config.lyrics.enabled_sources();
        let provider = self
            .config
            .resolve_connection(&self.config.lyrics.connection)
            .map_err(|_| ProviderError::Unavailable)
            .and_then(|route| {
                if let Some((cached_route, client)) = &self.lyrics_client {
                    if *cached_route == route
                        && client.iter().map(Provider::id).eq(sources.iter().copied())
                    {
                        return Ok(client.clone());
                    }
                }
                let client: Vec<_> = sources
                    .iter()
                    .map(|id| {
                        Provider::new(
                            *id,
                            &route,
                            self.lyrics_gate.entry(*id).or_default().clone(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                self.lyrics_client = Some((route, client.clone()));
                Ok(client)
            });
        LyricsService {
            settings: self.config.lyrics.clone(),
            provider,
            cache: self.lyrics_cache.clone(),
        }
    }

    pub(in crate::app) fn subscribe_lyrics_inputs(&mut self, cx: &mut Context<Self>) {
        for input in [
            &self.lyrics_ui.title,
            &self.lyrics_ui.artist,
            &self.lyrics_ui.album,
        ]
        .into_iter()
        .cloned()
        .collect::<Vec<_>>()
        {
            self._subscriptions
                .push(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. })
                        && this.lyrics_ui.dialog == Some(LyricsDialog::Search)
                    {
                        this.search_lyrics(cx);
                    }
                }));
        }
        let offset = self.lyrics_ui.offset.clone();
        self._subscriptions
            .push(cx.subscribe(&offset, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && this.lyrics_ui.dialog == Some(LyricsDialog::Timing)
                {
                    this.apply_lyrics_offset_input(cx);
                }
            }));
    }

    pub(in crate::app) fn open_lyrics_dialog(
        &mut self,
        kind: LyricsDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(song) = self.state.now_playing.clone() else {
            return;
        };
        let Some(media) = self.lyrics_media(&song) else {
            return;
        };
        self.lyrics_ui.cancel();
        self.lyrics_ui.close_requested = false;
        self.lyrics_ui.media = Some(media.clone());
        self.lyrics_ui.dialog = Some(kind);
        self.lyrics_ui.song_title = song.title.clone();
        let query = self
            .state
            .lyrics
            .document
            .as_ref()
            .and_then(|d| d.query.clone())
            .or_else(|| self.lyrics_cache.preference(&media).query)
            .unwrap_or_else(|| Query::for_song(&song));
        self.lyrics_ui.query = query.clone();
        self.lyrics_ui.records.clear();
        self.lyrics_ui.candidates_scroll = ScrollHandle::new();
        self.lyrics_ui.selected = None;
        self.lyrics_ui.error = None;
        self.lyrics_ui.source = None;
        self.lyrics_ui.failures.clear();
        self.lyrics_ui.searched = false;
        self.lyrics_ui.remember = true;
        self.lyrics_ui.plain = false;
        self.lyrics_ui
            .title
            .update(cx, |input, cx| input.set_value(query.title, window, cx));
        self.lyrics_ui
            .artist
            .update(cx, |input, cx| input.set_value(query.artist, window, cx));
        self.lyrics_ui
            .album
            .update(cx, |input, cx| input.set_value(query.album, window, cx));
        let offset = self
            .state
            .lyrics
            .document
            .as_ref()
            .map_or(0, |d| d.offset_ms);
        self.lyrics_ui.offset.update(cx, |input, cx| {
            input.set_value(offset.to_string(), window, cx)
        });
        let app = cx.entity().downgrade();
        let closing = app.clone();
        window.defer(cx, move |window, cx| {
            window.open_dialog(cx, move |dialog, window, cx| {
                let closing = closing.clone();
                app.update(cx, |this, cx| {
                    let dialog =
                        lyrics_dialog_behavior(this.render_lyrics_dialog(dialog, window, cx))
                            .on_close(move |_, _, cx| {
                                let _ = closing.update(cx, |this, cx| {
                                    this.lyrics_ui.cancel();
                                    this.lyrics_ui.close_requested = false;
                                    cx.notify();
                                });
                            });
                    crate::app::style::surface::raised_dialog(dialog, cx)
                })
                .expect("app owns lyrics dialog")
            });
        });
        cx.notify();
    }

    pub(in crate::app) fn sync_lyrics_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.lyrics_ui.dialog.is_some()
            && self.lyrics_ui.media.as_ref()
                != self
                    .state
                    .now_playing
                    .as_ref()
                    .and_then(|s| self.lyrics_media(s))
                    .as_ref()
        {
            self.lyrics_ui.cancel();
        }
        if std::mem::take(&mut self.lyrics_ui.close_requested) {
            window.defer(cx, |window, cx| {
                if window.has_active_dialog(cx) {
                    window.close_dialog(cx);
                }
            });
        }
    }

    pub(in crate::app) fn close_lyrics_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.lyrics_ui.cancel();
        self.lyrics_ui.close_requested = false;
        window.close_dialog(cx);
        cx.notify();
    }

    pub(in crate::app) fn search_lyrics(&mut self, cx: &mut Context<Self>) {
        if !self.config.lyrics.enabled || self.lyrics_ui.dialog != Some(LyricsDialog::Search) {
            return;
        }
        if self.config.lyrics.enabled_sources().is_empty() {
            self.lyrics_ui.error = Some(LyricsStatus::SourcesNeeded);
            cx.notify();
            return;
        }
        let Some(song) = self.state.now_playing.as_ref() else {
            return;
        };
        if self.lyrics_media(song).as_ref() != self.lyrics_ui.media.as_ref() {
            return;
        }
        let query = Query {
            title: self.lyrics_ui.title.read(cx).value().trim().to_string(),
            artist: self.lyrics_ui.artist.read(cx).value().trim().to_string(),
            album: self.lyrics_ui.album.read(cx).value().trim().to_string(),
            duration: Query::for_song(song).duration,
        };
        if query.title.is_empty() {
            self.lyrics_ui.error = Some(LyricsStatus::TitleNeeded);
            cx.notify();
            return;
        }
        if let Some(task) = self.lyrics_ui.task.take() {
            task.abort();
        }
        self.lyrics_ui.query = query.clone();
        self.lyrics_ui.records.clear();
        self.lyrics_ui.candidates_scroll = ScrollHandle::new();
        self.lyrics_ui.selected = None;
        self.lyrics_ui.error = None;
        self.lyrics_ui.failures.clear();
        let request = self.library_requests.next();
        self.lyrics_ui.request = Some(request);
        self.lyrics_ui.searched = true;
        let service = self.lyrics_service();
        let source = self.lyrics_ui.source;
        let tx = self.tx.clone();
        self.lyrics_ui.task = Some(self.runtime.spawn(async move {
            let result = service.search(&query, source).await;
            let _ = tx.send(Msg::LyricsSearch { request, result });
        }));
        cx.notify();
    }

    pub(in crate::app) fn finish_lyrics_search(
        &mut self,
        request: LibraryRequest,
        result: Result<SearchOutcome, ProviderError>,
    ) {
        if self.lyrics_ui.request != Some(request)
            || self.lyrics_ui.media.as_ref()
                != self
                    .state
                    .now_playing
                    .as_ref()
                    .and_then(|s| self.lyrics_media(s))
                    .as_ref()
        {
            return;
        }
        self.lyrics_ui.request = None;
        match result {
            Ok(outcome) => {
                self.lyrics_ui.records = matching::ranked(&self.lyrics_ui.query, outcome.records);
                self.lyrics_ui.failures = outcome
                    .failures
                    .into_iter()
                    .map(|(id, error)| (id, error.into()))
                    .collect();
                if self.lyrics_ui.records.is_empty() && self.lyrics_ui.failures.is_empty() {
                    self.lyrics_ui.error = Some(LyricsStatus::NoMatch);
                }
            }
            Err(error) => self.lyrics_ui.error = Some(error.into()),
        }
    }

    pub(in crate::app) fn apply_lyrics_candidate(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.lyrics_ui.request.is_some() {
            return;
        }
        let Some(song) = self.state.now_playing.as_ref() else {
            return;
        };
        let Some(media) = self
            .lyrics_media(song)
            .filter(|m| Some(m) == self.lyrics_ui.media.as_ref())
        else {
            return;
        };
        let Some(record) = self
            .lyrics_ui
            .selected
            .and_then(|i| self.lyrics_ui.records.get(i))
            .cloned()
        else {
            return;
        };
        let mut document = Document::online(record);
        document.manual = true;
        document.temporary = !self.lyrics_ui.remember;
        document.plain = self.lyrics_ui.plain;
        document.query = Some(self.lyrics_ui.query.clone());
        if self.lyrics_ui.remember {
            if let Err(error) = self.lyrics_cache.remember(&media, &document) {
                self.notify_error(t!("Could not save lyrics: %{error}", error = error));
                return;
            }
        }
        self.state.lyrics.apply(document);
        self.close_lyrics_dialog(window, cx);
    }

    pub(in crate::app) fn restore_lyrics_selection(&mut self, cx: &mut Context<Self>) {
        let Some(media) = self.state.lyrics.media().cloned() else {
            return;
        };
        if let Err(error) = self.lyrics_cache.restore(&media) {
            self.notify_error(t!("Could not save lyrics: %{error}", error = error));
            return;
        }
        // Restoring explicitly discards the selection. A failed automatic lookup
        // must not leave the removed lyrics displayed as an automatic match.
        self.state.lyrics.clear();
        self.refresh_lyrics();
        cx.notify();
    }

    pub(in crate::app) fn change_lyrics_display(
        &mut self,
        change: impl FnOnce(&mut Document),
        cx: &mut Context<Self>,
    ) {
        let Some(media) = self.state.lyrics.media().cloned() else {
            return;
        };
        let Some(mut document) = self.state.lyrics.document.clone() else {
            return;
        };
        change(&mut document);
        if !document.temporary {
            // A display preference for automatic content does not pin that content.
            let mut preference = document.clone();
            if !document.manual {
                preference.record = None;
            }
            if let Err(error) = self.lyrics_cache.remember(&media, &preference) {
                self.notify_error(t!("Could not save lyrics: %{error}", error = error));
                return;
            }
        }
        self.state.lyrics.apply(document);
        cx.notify();
    }

    pub(in crate::app) fn apply_lyrics_offset_input(&mut self, cx: &mut Context<Self>) {
        if let Ok(offset) = self.lyrics_ui.offset.read(cx).value().trim().parse::<i64>() {
            self.change_lyrics_display(|d| d.offset_ms = offset, cx);
        } else {
            self.notify_error(t!("Enter a whole number of milliseconds."));
        }
    }
}

#[cfg(test)]
mod tests;
