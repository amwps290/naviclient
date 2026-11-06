//! Navigation state transitions, overlay priority, and library-loading dispatch.

use crate::app::prelude::*;

impl AppState {
    /// Change the browsing page without dismissing the queue or its selection.
    pub(in crate::app) fn navigate_to(&mut self, view: View) {
        self.settings.open = false;
        self.view = view;
    }

    pub(in crate::app) fn open_now_playing(&mut self) {
        if self.view != View::NowPlaying {
            self.view_before_now_playing = self.view;
        }
        self.navigate_to(View::NowPlaying);
        self.lyrics.reset_follow();
    }

    pub(in crate::app) fn leave_now_playing(&mut self) {
        self.view = self.view_before_now_playing;
        self.lyrics.reset_follow();
    }

    /// Called only after shortcut recording and input/dialog/menu focus guards.
    /// Escape dismisses one layer, keeping the underlying browsing context.
    pub(in crate::app) fn close_top_overlay(&mut self) {
        if self.song_selection.multi().is_some() {
            self.song_selection.clear_multi();
        } else if self.settings.open {
            self.settings.open = false;
        } else if self.queue_drawer_open {
            self.queue_drawer_open = false;
            self.song_selection.clear_multi();
        } else if self.view == View::NowPlaying {
            self.leave_now_playing();
        } else if let Some(parent) = self.view.detail_parent() {
            self.view = parent;
        }
    }
}

impl NavidromeApp {
    pub(in crate::app) fn navigate_to(&mut self, view: View) {
        self.state.navigate_to(view);
        match view {
            View::Artists if self.state.artists.is_empty() => self.load_artists(),
            View::Albums if self.state.albums.is_empty() => self.load_albums(),
            View::Genres if self.state.genres.is_empty() => self.load_genres(),
            View::Playlists if self.state.playlists.items().is_empty() => self.load_playlists(),
            View::Favorites => self.load_favorites(),
            _ => {}
        }
    }

    pub(in crate::app) fn open_now_playing(&mut self) {
        self.state.open_now_playing();
    }

    pub(in crate::app) fn leave_now_playing(&mut self) {
        self.state.leave_now_playing();
    }
}

#[cfg(test)]
mod tests;
