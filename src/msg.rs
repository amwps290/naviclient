use crate::models::{
    Album, Artist, FavoriteKey, Favorites, Lyrics, Playlist, SearchResults, ServerInfo, Song,
};

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
    Favorites(Result<Favorites, String>),
    FavoriteChanged {
        key: FavoriteKey,
        starred: bool,
        result: Result<(), String>,
    },
    PlaylistSongs {
        playlist_id: String,
        result: Result<Vec<Song>, String>,
    },
    Search(Result<SearchResults, String>),
    Lyrics {
        song_id: String,
        result: Result<Lyrics, String>,
    },
    Cover {
        id: String,
        result: Result<Vec<u8>, String>,
    },
    CoverRotationFrames {
        id: String,
        result: Result<Vec<Vec<u8>>, String>,
    },
}

pub fn error_message(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}
