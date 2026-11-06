//! 应用运行时状态、内部状态类型及默认初始化。

use super::album_details::AlbumDetails;
use super::artist_details::ArtistDetails;
use super::artist_library::ArtistLibrary;
use super::audio_output::OutputDevices;
use super::auto_continue_state::AutoContinueState;
use super::cover_state::CoverCache;
use super::daily_mix_state::DailyMixState;
use super::favorites::FavoritesState;
use super::genre_library::GenreLibrary;
use super::home_library::HomeAlbums;
use super::library_playback::LibraryPlayback;
use super::listening_state::ListeningState;
use super::loudness_state::LoudnessCache;
use super::lyrics_state::LyricsState;
use super::paged_collection::PagedCollection;
use super::playback_occurrence::PlaybackOccurrence;
use super::playlist_library::PlaylistLibrary;
use super::queue_logic::ShuffleHistory;
use super::radio_state::RadioState;
use super::search_state::SearchState;
use super::server_connection::ServerConnection;
use super::settings_state::SettingsUi;
use super::song_selection::SongSelection;
use super::*;

pub(super) struct AppState {
    pub(super) connection: ServerConnection,
    pub(super) view: View,
    pub(super) artists: ArtistLibrary,
    pub(super) albums: PagedCollection<Album>,
    pub(super) albums_sort: AlbumSort,
    pub(super) home_albums: HomeAlbums,
    pub(super) daily_mix: DailyMixState,
    pub(super) auto_continue: AutoContinueState,
    pub(super) radio: RadioState,
    pub(super) genres: GenreLibrary,
    pub(super) search: SearchState,
    pub(super) collapsed_sections: HashSet<String>,
    pub(super) artist_details: ArtistDetails,
    pub(super) album_details: AlbumDetails,
    pub(super) library_playback: LibraryPlayback,
    pub(super) playlists: PlaylistLibrary,
    pub(super) favorites: FavoritesState,
    pub(super) hovered_item: Option<String>,
    pub(super) queue: PlaybackQueue,
    pub(super) queue_index: Option<usize>,
    /// 右侧 Up Next / History 抽屉；不再用独立页面替换当前浏览上下文。
    pub(super) queue_drawer_open: bool,
    pub(super) queue_drawer_tab: QueueDrawerTab,
    pub(super) now_playing: Option<Song>,
    pub(super) now_playing_quality: Option<TranscodingQuality>,
    pub(super) lyrics: LyricsState,
    pub(super) covers: CoverCache,
    pub(super) status: String,
    pub(super) settings: SettingsUi,
    /// Scoped cursor, multi-selection, and fixed range anchor.
    pub(super) song_selection: SongSelection,
    /// “曲目信息”对话框当前展示的歌曲。
    pub(super) song_info: Option<Song>,
    pub(super) view_before_now_playing: View,
    pub(super) playback_modes: PlayModes,
    pub(super) shuffle_history: ShuffleHistory,
    pub(super) occurrence: PlaybackOccurrence,
    pub(super) output_devices: OutputDevices,
    /// 本地收听事件和 History 列表；与播放会话一样按服务器隔离。
    pub(super) listening: ListeningState,
    /// 本地测量响度、所属服务器和播放增益选择。
    pub(super) loudness: LoudnessCache,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connection: ServerConnection::default(),
            view: View::Home,
            artists: ArtistLibrary::default(),
            albums: PagedCollection::default(),
            albums_sort: AlbumSort::default(),
            home_albums: HomeAlbums::default(),
            daily_mix: DailyMixState::default(),
            auto_continue: AutoContinueState::default(),
            radio: RadioState::default(),
            genres: GenreLibrary::default(),
            search: SearchState::default(),
            collapsed_sections: HashSet::from(["random".to_string()]),
            artist_details: ArtistDetails::default(),
            album_details: AlbumDetails::default(),
            library_playback: LibraryPlayback::default(),
            playlists: PlaylistLibrary::default(),
            favorites: FavoritesState::default(),
            hovered_item: None,
            queue: PlaybackQueue::default(),
            queue_index: None,
            queue_drawer_open: false,
            queue_drawer_tab: QueueDrawerTab::default(),
            now_playing: None,
            now_playing_quality: None,
            lyrics: LyricsState::default(),
            covers: CoverCache::default(),
            status: t!("Not connected").to_string(),
            settings: SettingsUi::default(),
            song_selection: SongSelection::default(),
            song_info: None,
            view_before_now_playing: View::Home,
            playback_modes: PlayModes::default(),
            shuffle_history: ShuffleHistory::default(),
            occurrence: PlaybackOccurrence::default(),
            output_devices: OutputDevices::default(),
            listening: ListeningState::default(),
            loudness: LoudnessCache::default(),
        }
    }
}
