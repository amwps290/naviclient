//! Typed song-list identity, stable element IDs, and shared presentation rules.

use std::fmt;

use crate::app::shell::view_state::{QueueDrawerTab, View};
use crate::models::QueueOrigin;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::app) enum SongScope {
    AlbumDetail,
    ArtistTop,
    PlaylistDetail,
    Favorites,
    Search,
    Queue,
    History,
}

impl SongScope {
    pub(in crate::app) const fn id(self) -> &'static str {
        match self {
            Self::AlbumDetail => "album-detail",
            Self::ArtistTop => "artist-top",
            Self::PlaylistDetail => "playlist-detail",
            Self::Favorites => "favorites",
            Self::Search => "search",
            Self::Queue => "queue",
            Self::History => "history",
        }
    }

    pub(in crate::app) fn active(
        view: View,
        drawer_open: bool,
        tab: QueueDrawerTab,
    ) -> Option<Self> {
        if drawer_open {
            return Some(match tab {
                QueueDrawerTab::UpNext => Self::Queue,
                QueueDrawerTab::History => Self::History,
            });
        }
        match view {
            View::AlbumDetail => Some(Self::AlbumDetail),
            View::ArtistDetail => Some(Self::ArtistTop),
            View::PlaylistDetail => Some(Self::PlaylistDetail),
            View::Favorites => Some(Self::Favorites),
            View::Search => Some(Self::Search),
            _ => None,
        }
    }

    pub(in crate::app) fn is_drawer(self) -> bool {
        matches!(self, Self::Queue | Self::History)
    }

    pub(in crate::app) fn fills_viewport(self) -> bool {
        matches!(
            self,
            Self::AlbumDetail | Self::PlaylistDetail | Self::Queue | Self::History
        )
    }

    pub(in crate::app) fn queue_origin(self) -> QueueOrigin {
        match self {
            Self::PlaylistDetail => QueueOrigin::Playlist,
            _ => QueueOrigin::Manual,
        }
    }

    #[cfg(test)]
    pub(in crate::app) const ALL: [Self; 7] = [
        Self::AlbumDetail,
        Self::ArtistTop,
        Self::PlaylistDetail,
        Self::Favorites,
        Self::Search,
        Self::Queue,
        Self::History,
    ];
}

/// Existing list and toolbar element IDs use this stable spelling, not Debug.
impl fmt::Display for SongScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests;
