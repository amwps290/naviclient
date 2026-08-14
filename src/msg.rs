use std::sync::Arc;

use gpui::{Hsla, Image as GpuiImage};

use crate::models::{
    Album, Artist, FavoriteKey, Favorites, Lyrics, Playlist, SearchResults, ServerInfo, Song,
};

/// 后台线程解码完成的封面结果，UI 线程只负责插入缓存。
#[derive(Debug)]
pub struct DecodedCover {
    pub palette: Option<(Hsla, Hsla)>,
    pub image: Option<Arc<GpuiImage>>,
}

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
        result: Result<DecodedCover, String>,
    },
}

pub fn error_message(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}
