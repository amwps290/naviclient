//! Browsing navigation history stack (Back / Forward) with per-entry scroll offset.

use crate::models::{Album, Artist, Playlist};

const MAX_HISTORY_LEN: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum NavLocation {
    Home,
    Favorites,
    Artists,
    Albums,
    Genres,
    Folders,
    Playlists,
    Search,
    ArtistDetail { id: String, artist: Artist },
    AlbumDetail { id: String, album: Album },
    GenreDetail(String),
    PlaylistDetail { id: String, playlist: Playlist },
    LocalPlaylistDetail { id: String },
}

impl NavLocation {
    pub(in crate::app) fn artist(artist: Artist) -> Self {
        Self::ArtistDetail {
            id: artist.id.clone(),
            artist,
        }
    }

    pub(in crate::app) fn album(album: Album) -> Self {
        Self::AlbumDetail {
            id: album.id.clone(),
            album,
        }
    }

    pub(in crate::app) fn playlist(playlist: Playlist) -> Self {
        Self::PlaylistDetail {
            id: playlist.id.clone(),
            playlist,
        }
    }

    pub(in crate::app) fn local_playlist(id: impl Into<String>) -> Self {
        Self::LocalPlaylistDetail { id: id.into() }
    }

    fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Home, Self::Home)
            | (Self::Favorites, Self::Favorites)
            | (Self::Artists, Self::Artists)
            | (Self::Albums, Self::Albums)
            | (Self::Genres, Self::Genres)
            | (Self::Folders, Self::Folders)
            | (Self::Playlists, Self::Playlists)
            | (Self::Search, Self::Search) => true,
            (Self::ArtistDetail { id: a, .. }, Self::ArtistDetail { id: b, .. })
            | (Self::AlbumDetail { id: a, .. }, Self::AlbumDetail { id: b, .. })
            | (Self::PlaylistDetail { id: a, .. }, Self::PlaylistDetail { id: b, .. })
            | (Self::LocalPlaylistDetail { id: a }, Self::LocalPlaylistDetail { id: b })
            | (Self::GenreDetail(a), Self::GenreDetail(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::app) struct NavEntry {
    pub(in crate::app) location: NavLocation,
    pub(in crate::app) scroll_y: f32,
}

#[derive(Clone, Debug)]
pub(in crate::app) struct NavHistory {
    back: Vec<NavEntry>,
    current: NavEntry,
    forward: Vec<NavEntry>,
}

impl Default for NavHistory {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            current: NavEntry {
                location: NavLocation::Home,
                scroll_y: 0.0,
            },
            forward: Vec::new(),
        }
    }
}

impl NavHistory {
    pub(in crate::app) fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub(in crate::app) fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub(in crate::app) fn push(&mut self, location: NavLocation, current_scroll_y: f32) {
        if self.current.location.same_target(&location) {
            self.current.location = location;
            return;
        }
        let mut previous = std::mem::replace(
            &mut self.current,
            NavEntry {
                location,
                scroll_y: 0.0,
            },
        );
        previous.scroll_y = current_scroll_y;
        self.back.push(previous);
        if self.back.len() > MAX_HISTORY_LEN {
            let overflow = self.back.len() - MAX_HISTORY_LEN;
            self.back.drain(0..overflow);
        }
        self.forward.clear();
    }

    pub(in crate::app) fn go_back(&mut self, current_scroll_y: f32) -> Option<NavEntry> {
        let target = self.back.pop()?;
        let mut outgoing = std::mem::replace(&mut self.current, target.clone());
        outgoing.scroll_y = current_scroll_y;
        self.forward.push(outgoing);
        Some(target)
    }

    pub(in crate::app) fn go_forward(&mut self, current_scroll_y: f32) -> Option<NavEntry> {
        let target = self.forward.pop()?;
        let mut outgoing = std::mem::replace(&mut self.current, target.clone());
        outgoing.scroll_y = current_scroll_y;
        self.back.push(outgoing);
        Some(target)
    }

    pub(in crate::app) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests;
