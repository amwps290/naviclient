//! Navigation state transitions, overlay priority, and library-loading dispatch.

use super::history::{NavEntry, NavLocation};
use crate::app::prelude::*;

impl AppState {
    /// Change the browsing page without dismissing the queue or its selection.
    pub(in crate::app) fn navigate_to(&mut self, view: View) {
        self.settings.close();
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
            self.settings.close();
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
    fn current_scroll_y(&self) -> f32 {
        f32::from(self.content_scroll_handle.offset().y)
    }

    pub(in crate::app) fn record_nav_location(&mut self, location: NavLocation) {
        let scroll_y = self.current_scroll_y();
        self.state.nav_history.push(location, scroll_y);
    }

    pub(in crate::app) fn navigate_to(&mut self, view: View) {
        let view = if self.browsing_bilibili()
            && !matches!(
                view,
                View::Home
                    | View::Playlists
                    | View::PlaylistDetail
                    | View::Search
                    | View::NowPlaying
            ) {
            View::Playlists
        } else {
            view
        };
        if let Some(location) = top_level_nav_location(view) {
            self.record_nav_location(location);
        }
        self.enter_top_level_view(view);
    }

    fn enter_top_level_view(&mut self, view: View) {
        if self.browsing_bilibili() && view == View::Search {
            self.state.queue_drawer_open = false;
        }
        self.state.navigate_to(view);
        match view {
            View::Artists if self.state.artists.is_empty() => self.load_artists(),
            View::Albums if self.state.albums.is_empty() => self.load_albums(),
            View::Genres if self.state.genres.is_empty() => self.load_genres(),
            View::Folders if !self.state.folders.loaded_once() => self.load_current_folder(),
            View::Playlists if self.state.playlists.items().is_empty() => self.load_playlists(),
            View::Favorites => self.load_favorites(),
            _ => {}
        }
    }

    pub(in crate::app) fn navigate_back(&mut self) -> bool {
        let scroll_y = self.current_scroll_y();
        let Some(entry) = self.state.nav_history.go_back(scroll_y) else {
            return false;
        };
        self.restore_nav_entry(entry);
        true
    }

    pub(in crate::app) fn navigate_forward(&mut self) -> bool {
        let scroll_y = self.current_scroll_y();
        let Some(entry) = self.state.nav_history.go_forward(scroll_y) else {
            return false;
        };
        self.restore_nav_entry(entry);
        true
    }

    fn restore_nav_entry(&mut self, entry: NavEntry) {
        match entry.location {
            NavLocation::Home => self.enter_top_level_view(View::Home),
            NavLocation::Favorites => self.enter_top_level_view(View::Favorites),
            NavLocation::Artists => self.enter_top_level_view(View::Artists),
            NavLocation::Albums => self.enter_top_level_view(View::Albums),
            NavLocation::Genres => self.enter_top_level_view(View::Genres),
            NavLocation::Folders => self.enter_top_level_view(View::Folders),
            NavLocation::Playlists => self.enter_top_level_view(View::Playlists),
            NavLocation::Search => self.enter_top_level_view(View::Search),
            NavLocation::ArtistDetail { artist, .. } => {
                if self
                    .state
                    .artist_details
                    .artist()
                    .is_some_and(|current| current.id == artist.id)
                {
                    self.state.navigate_to(View::ArtistDetail);
                } else {
                    self.open_artist_without_history(artist);
                }
            }
            NavLocation::AlbumDetail { album, .. } => {
                if self
                    .state
                    .album_details
                    .album()
                    .is_some_and(|current| current.id == album.id)
                {
                    self.state.navigate_to(View::AlbumDetail);
                } else {
                    self.open_album_without_history(album);
                }
            }
            NavLocation::GenreDetail(genre) => {
                if self
                    .state
                    .genres
                    .detail()
                    .is_some_and(|detail| detail.name() == genre)
                {
                    self.state.navigate_to(View::GenreDetail);
                } else {
                    self.open_genre_without_history(genre);
                }
            }
            NavLocation::PlaylistDetail { playlist, .. } => {
                if self
                    .state
                    .playlists
                    .playlist()
                    .is_some_and(|current| current.id == playlist.id)
                {
                    self.state.navigate_to(View::PlaylistDetail);
                } else {
                    self.open_playlist_without_history(playlist);
                }
            }
        }
        self.content_scroll_handle
            .set_offset(point(px(0.0), px(entry.scroll_y)));
    }

    pub(in crate::app) fn open_now_playing(&mut self) {
        self.state.open_now_playing();
    }

    pub(in crate::app) fn leave_now_playing(&mut self) {
        self.state.leave_now_playing();
    }
}

fn top_level_nav_location(view: View) -> Option<NavLocation> {
    Some(match view {
        View::Home => NavLocation::Home,
        View::Favorites => NavLocation::Favorites,
        View::Artists => NavLocation::Artists,
        View::Albums => NavLocation::Albums,
        View::Genres => NavLocation::Genres,
        View::Folders => NavLocation::Folders,
        View::Playlists => NavLocation::Playlists,
        View::Search => NavLocation::Search,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
