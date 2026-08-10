use crate::models::{Album, Artist, Playlist, SearchResults, ServerInfo, Song};

#[derive(Debug)]
pub enum Msg {
    Ping(Result<ServerInfo, String>),
    Artists(Result<Vec<Artist>, String>),
    ArtistAlbums {
        artist_id: String,
        result: Result<Vec<Album>, String>,
    },
    Albums(Result<Vec<Album>, String>),
    AlbumSongs {
        album_id: String,
        result: Result<Vec<Song>, String>,
    },
    Playlists(Result<Vec<Playlist>, String>),
    PlaylistSongs {
        playlist_id: String,
        result: Result<Vec<Song>, String>,
    },
    Search(Result<SearchResults, String>),
    Cover {
        id: String,
        result: Result<Vec<u8>, String>,
    },
}

pub fn error_message(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}
