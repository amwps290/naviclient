use std::sync::Arc;

use gpui_kit::{Hsla, Image as GpuiImage};

use crate::app::{
    AutoContinueRequest, CoverRequest, DailyMixRequest, FavoriteChange, HomeAlbumSection,
    LibraryPlaybackRequest, LibraryRequest, PageRequest, PlaybackResolveRequest, RadioRequest,
};
use crate::audio::OutputDevice;
use crate::import::{ImportedPlaylist, MatchedSongs};
use crate::models::{
    Album, Artist, Favorites, Genre, Lyrics, Playlist, SearchResults, ServerInfo, Song,
};
use crate::playlist::{PlaylistDetails, PlaylistEdited};
use crate::recommendation::{DailyMixSnapshot, RecommendedTrack};

#[derive(Debug)]
pub(crate) struct PlaybackResolveError {
    pub(crate) message: String,
    pub(crate) unavailable: bool,
    pub(crate) login_expired: bool,
}

/// 后台线程解码完成的封面结果，UI 线程只负责插入缓存。
#[derive(Debug)]
pub struct DecodedCover {
    pub palette: Option<(Hsla, Hsla)>,
    pub image: Option<Arc<GpuiImage>>,
}

#[derive(Debug)]
pub enum Msg {
    BilibiliLogin {
        request: u128,
        update: crate::app::settings::bilibili_state::LoginUpdate,
    },
    BilibiliAccount {
        source: crate::source::SourceId,
        result: Result<
            crate::source::bilibili::auth::AuthSession,
            crate::app::settings::bilibili_state::LoginError,
        >,
    },
    BilibiliRemoved {
        source: crate::source::SourceId,
        result: Result<(), crate::secrets::source_credentials::CredentialError>,
    },
    PlaybackResolved {
        request: PlaybackResolveRequest,
        result: Result<crate::source::PlaybackResource, PlaybackResolveError>,
    },
    PreloadResolved {
        request: PlaybackResolveRequest,
        result: Result<crate::source::PlaybackResource, PlaybackResolveError>,
    },
    Ping {
        request: LibraryRequest,
        result: Result<ServerInfo, String>,
    },
    OpenSubsonicInfo {
        request: LibraryRequest,
        result: Result<ServerInfo, String>,
    },
    Artists {
        request: LibraryRequest,
        result: Result<Vec<Artist>, String>,
    },
    ArtistAlbums {
        request: LibraryRequest,
        result: Result<Vec<Album>, String>,
    },
    ArtistTopSongs {
        request: LibraryRequest,
        result: Result<Vec<Song>, String>,
    },
    Albums {
        request: PageRequest,
        result: Result<Vec<Album>, String>,
    },
    HomeAlbums {
        section: HomeAlbumSection,
        request: LibraryRequest,
        result: Result<Vec<Album>, String>,
    },
    Genres {
        request: LibraryRequest,
        result: Result<Vec<Genre>, String>,
    },
    OutputDevices {
        request: LibraryRequest,
        result: Result<Vec<OutputDevice>, String>,
    },
    GenreAlbums {
        request: PageRequest,
        result: Result<Vec<Album>, String>,
    },
    DailyMix {
        request: DailyMixRequest,
        result: Result<DailyMixSnapshot, String>,
    },
    AlbumSongs {
        request: LibraryRequest,
        result: Result<Vec<Song>, String>,
    },
    Playlists {
        request: LibraryRequest,
        result: Result<Vec<Playlist>, String>,
    },
    PlaylistEdited {
        request: LibraryRequest,
        result: Result<PlaylistEdited, String>,
    },
    Favorites {
        request: LibraryRequest,
        result: Result<Favorites, String>,
    },
    FavoriteChanged {
        change: FavoriteChange,
        result: Result<(), String>,
    },
    PlaylistSongs {
        request: LibraryRequest,
        result: Result<PlaylistDetails, String>,
    },
    Search {
        request: LibraryRequest,
        result: Result<SearchResults, String>,
    },
    LibraryPlayback {
        request: LibraryPlaybackRequest,
        result: Result<Vec<Song>, String>,
    },
    AutoContinue {
        request: AutoContinueRequest,
        result: Result<Vec<RecommendedTrack>, String>,
    },
    Radio {
        request: RadioRequest,
        result: Result<Vec<RecommendedTrack>, String>,
    },
    Lyrics {
        request: LibraryRequest,
        result: Result<Lyrics, String>,
    },
    Cover {
        request: CoverRequest,
        result: Result<DecodedCover, String>,
    },
    /// 外部歌单链接解析 + 拉取完成（app/import/orchestration.rs 的第一阶段）。
    ImportResolved {
        request: LibraryRequest,
        result: Result<ImportedPlaylist, String>,
    },
    /// 匹配进度（每完成一首一条，代价很小，UI 循环 50ms 消费）。
    ImportProgress {
        request: LibraryRequest,
        done: usize,
    },
    /// 匹配完成（第二阶段）：结果与导入曲目一一对应。
    ImportMatched {
        request: LibraryRequest,
        result: Result<MatchedSongs, String>,
    },
}

pub fn error_message(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}
