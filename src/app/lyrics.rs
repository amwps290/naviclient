//! 歌词请求、状态管理和同步歌词滚动跟随。

use super::*;

impl NavidromeApp {
    pub(super) fn load_lyrics(&mut self, song: &Song) {
        self.active_lyric_index = None;
        if self.state.lyrics_song_id.as_deref() == Some(song.id.as_str())
            && (self.state.lyrics.is_some() || self.state.lyrics_loading)
        {
            return;
        }

        self.state.lyrics_song_id = Some(song.id.clone());
        self.state.lyrics = None;
        self.state.lyrics_error = None;
        let Some(api) = self.api.clone() else {
            self.state.lyrics_loading = false;
            self.state.lyrics_error = Some(t!("Configure a server to load lyrics").to_string());
            return;
        };

        self.state.lyrics_loading = true;
        let structured_lyrics = self
            .state
            .server
            .as_ref()
            .map(|server| server.capabilities.structured_lyrics)
            .unwrap_or_default();
        let song = song.clone();
        let song_id = song.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .lyrics(&song, structured_lyrics)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::Lyrics { song_id, result });
        });
    }

    pub(super) fn update_active_lyric(&mut self) {
        if self.state.view != View::NowPlaying {
            self.lyrics_scroll_target = None;
            return;
        }

        let active = self
            .state
            .lyrics
            .as_ref()
            .filter(|lyrics| lyrics.is_synced())
            .and_then(|lyrics| lyrics.active_line_index(self.audio.state().position));

        if active != self.active_lyric_index {
            self.active_lyric_index = active;
            if let Some(index) = active {
                let item_index = index + 1;
                if let Some(item_bounds) = self.lyrics_scroll_handle.bounds_for_item(item_index) {
                    let viewport_bounds = self.lyrics_scroll_handle.bounds();
                    self.lyrics_scroll_target =
                        Some(viewport_bounds.center().y - item_bounds.center().y);
                } else {
                    self.lyrics_scroll_handle.scroll_to_item(item_index);
                }
            }
        }

        if let Some(target) = self.lyrics_scroll_target {
            let current = self.lyrics_scroll_handle.offset();
            let distance = f32::from(target - current.y);
            if distance.abs() <= 0.75 {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, target));
                self.lyrics_scroll_target = None;
            } else {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, current.y + px(distance * 0.24)));
            }
        }
    }
}
