//! 歌词请求、状态管理和同步歌词滚动跟随。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn load_lyrics(&mut self, song: &Song) {
        let Some(media) = self.lyrics_media(song) else {
            self.state.lyrics.clear();
            return;
        };
        if self.state.lyrics.media() != Some(&media)
            || self
                .lyrics_ui
                .media
                .as_ref()
                .is_some_and(|target| target != &media)
        {
            self.lyrics_ui.cancel();
        }
        let Some(request) = self.state.lyrics.begin(&media, &mut self.library_requests) else {
            return;
        };
        self.start_lyrics_load(song, media, request, false);
    }

    pub(in crate::app) fn lyrics_media(&self, song: &Song) -> Option<crate::source::MediaKey> {
        song.media_key().or_else(|| {
            self.source_for_song(song)
                .map(|api| crate::source::MediaKey::new(api.id().clone(), &song.id))
        })
    }

    pub(in crate::app) fn refresh_lyrics(&mut self) {
        if self
            .state
            .lyrics
            .document
            .as_ref()
            .is_some_and(|d| d.manual)
        {
            self.state.lyrics.cancel_task();
            return;
        }
        let Some(song) = self.state.now_playing.clone() else {
            return;
        };
        let Some(media) = self.lyrics_media(&song) else {
            return;
        };
        let request = self
            .state
            .lyrics
            .refresh(&media, &mut self.library_requests);
        self.start_lyrics_load(&song, media, request, true);
    }

    fn start_lyrics_load(
        &mut self,
        song: &Song,
        media: crate::source::MediaKey,
        request: LibraryRequest,
        refresh: bool,
    ) {
        let api = self.source_for_song(song).cloned();
        let service = self.lyrics_service();
        let capabilities = self
            .playback_server
            .as_ref()
            .map(|server| server.capabilities)
            .unwrap_or_default();
        let song = song.clone();
        let tx = self.tx.clone();
        self.state.lyrics.task = Some(self.runtime.spawn(async move {
            let result = service
                .load(
                    &song,
                    &media,
                    api.as_ref(),
                    capabilities,
                    refresh,
                    |status, _| {
                        let _ = tx.send(Msg::LyricsProgress { request, status });
                    },
                )
                .await;
            let _ = tx.send(Msg::Lyrics { request, result });
        }));
    }

    pub(in crate::app) fn update_active_lyric(&mut self) {
        if !self.fullscreen.is_active()
            && (self.state.view != View::NowPlaying || self.mini_window.is_mini())
        {
            self.state.lyrics.scroll_target = None;
            return;
        }
        let active = self
            .state
            .lyrics
            .lyrics()
            .filter(|lyrics| lyrics.is_synced())
            .and_then(|lyrics| lyrics.display_line_index(self.playback_for_display().position));
        let scroll = if self.fullscreen.is_active() {
            self.fullscreen.scroll_handle()
        } else {
            &self.lyrics_scroll_handle
        };
        follow_lyrics(
            &mut self.state.lyrics,
            scroll,
            active,
            self.lyrics_ui.timing.is_aligning(),
            std::time::Instant::now(),
        );
    }

    pub(in crate::app) fn copy_text_to_clipboard(
        &mut self,
        text: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.into()));
        self.notify_success(t!("Copied to clipboard"));
        cx.notify();
    }

    pub(in crate::app) fn copy_all_lyrics(&mut self, cx: &mut Context<Self>) {
        let Some(lyrics) = self.state.lyrics.lyrics() else {
            return;
        };
        let include_translation = self.config.show_lyrics_translation;
        let text = format_lyrics_plain(lyrics, include_translation);
        if text.is_empty() {
            return;
        }
        self.copy_text_to_clipboard(text, cx);
    }
}

pub(in crate::app) fn format_lyrics_plain(
    lyrics: &crate::models::Lyrics,
    include_translation: bool,
) -> String {
    let mut lines = Vec::new();
    for line in &lyrics.lines {
        let main = line.text.trim();
        if main.is_empty() {
            continue;
        }
        lines.push(main.to_string());
        if include_translation {
            if let Some(translation) = line
                .translation
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                lines.push(translation.to_string());
            }
        }
    }
    lines.join("\n")
}

/// Follow live cues from the current viewport, with a pause while a line is
/// picked for alignment or after manual wheel scrolling. Resuming rebuilds the
/// target even within the same cue.
fn follow_lyrics(
    lyrics: &mut super::lyrics_state::LyricsState,
    scroll: &ScrollHandle,
    active: Option<usize>,
    aligning: bool,
    now: std::time::Instant,
) {
    if aligning {
        lyrics.scroll_target = None;
        lyrics.follow_resume = true;
        return;
    }

    // A new document can have no started cue during its intro. Reset the
    // actual viewport even when both the old and new active indices are None.
    if std::mem::take(&mut lyrics.follow_reset) {
        scroll.set_offset(point(px(0.0), px(0.0)));
    }
    if lyrics.is_manual_scroll_paused(now) {
        lyrics.scroll_target = None;
        return;
    }
    if lyrics.manual_scroll_until.is_some() {
        lyrics.manual_scroll_until = None;
        lyrics.follow_resume = true;
    }
    let resumed = std::mem::take(&mut lyrics.follow_resume);
    if active != lyrics.active_index || resumed {
        lyrics.active_index = active;
        lyrics.scroll_target = None;
        if let Some(index) = active {
            let item_index = index + 1;
            if let Some(item_bounds) = scroll.bounds_for_item(item_index) {
                let viewport_bounds = scroll.bounds();
                lyrics.scroll_target = Some(viewport_bounds.center().y - item_bounds.center().y);
            } else {
                scroll.scroll_to_item(item_index);
            }
        } else {
            // Seeking backwards into the intro must also leave the outro.
            scroll.set_offset(point(px(0.0), px(0.0)));
        }
    }

    if let Some(target) = lyrics.scroll_target {
        let current = scroll.offset();
        let distance = f32::from(target - current.y);
        if distance.abs() <= 0.75 {
            scroll.set_offset(point(current.x, target));
            lyrics.scroll_target = None;
        } else {
            scroll.set_offset(point(current.x, current.y + px(distance * 0.24)));
        }
    }
}

#[cfg(test)]
mod tests;
