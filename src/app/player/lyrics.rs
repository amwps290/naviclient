//! 歌词请求、状态管理和同步歌词滚动跟随。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn load_lyrics(&mut self, song: &Song) {
        let Some(api) = self
            .source_for_song(song)
            .cloned()
            .filter(|api| api.capabilities().lyrics)
        else {
            self.state.lyrics.clear();
            return;
        };
        let media = crate::source::MediaKey::new(api.id().clone(), &song.id);
        let Some(request) = self.state.lyrics.begin(&media, &mut self.library_requests) else {
            return;
        };
        let structured_lyrics = self
            .playback_server
            .as_ref()
            .map(|server| server.capabilities.structured_lyrics)
            .unwrap_or_default();
        let song = song.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .lyrics(&song, structured_lyrics)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::Lyrics { request, result });
        });
    }

    pub(in crate::app) fn update_active_lyric(&mut self) {
        if self.state.view != View::NowPlaying {
            self.state.lyrics.scroll_target = None;
            return;
        }

        let active = self
            .state
            .lyrics
            .lyrics()
            .filter(|lyrics| lyrics.is_synced())
            .and_then(|lyrics| lyrics.active_line_index(self.playback_for_display().position));

        if active != self.state.lyrics.active_index {
            self.state.lyrics.active_index = active;
            self.state.lyrics.scroll_target = None;
            if let Some(index) = active {
                let item_index = index + 1;
                if let Some(item_bounds) = self.lyrics_scroll_handle.bounds_for_item(item_index) {
                    let viewport_bounds = self.lyrics_scroll_handle.bounds();
                    self.state.lyrics.scroll_target =
                        Some(viewport_bounds.center().y - item_bounds.center().y);
                } else {
                    self.lyrics_scroll_handle.scroll_to_item(item_index);
                }
            }
        }

        if let Some(target) = self.state.lyrics.scroll_target {
            let current = self.lyrics_scroll_handle.offset();
            let distance = f32::from(target - current.y);
            if distance.abs() <= 0.75 {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, target));
                self.state.lyrics.scroll_target = None;
            } else {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, current.y + px(distance * 0.24)));
            }
        }
    }
}
