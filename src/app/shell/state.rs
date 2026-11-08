//! 应用运行时状态、内部状态类型及默认初始化。

use crate::app::covers::state::CoverCache;
use crate::app::home::albums::HomeAlbums;
use crate::app::library::album_details::AlbumDetails;
use crate::app::library::artist_details::ArtistDetails;
use crate::app::library::artist_library::ArtistLibrary;
use crate::app::library::favorites::FavoritesState;
use crate::app::library::genre_library::GenreLibrary;
use crate::app::library::paged_collection::PagedCollection;
use crate::app::library::playback::LibraryPlayback;
use crate::app::playback::listening_state::ListeningState;
use crate::app::playback::occurrence::PlaybackOccurrence;
use crate::app::player::lyrics_state::LyricsState;
use crate::app::playlists::library::PlaylistLibrary;
use crate::app::prelude::*;
use crate::app::queue::logic::ShuffleHistory;
use crate::app::recommendations::auto_continue_state::AutoContinueState;
use crate::app::recommendations::daily_mix_state::DailyMixState;
use crate::app::recommendations::radio_state::RadioState;
use crate::app::search::state::SearchState;
use crate::app::server::connection_state::ServerConnection;
use crate::app::settings::state::SettingsUi;
use crate::app::songs::selection::SongSelection;
use crate::app::sound::loudness_state::LoudnessCache;
use crate::app::sound::output::OutputDevices;

pub(in crate::app) struct AppState {
    pub(in crate::app) connection: ServerConnection,
    pub(in crate::app) view: View,
    pub(in crate::app) artists: ArtistLibrary,
    pub(in crate::app) albums: PagedCollection<Album>,
    pub(in crate::app) albums_sort: AlbumSort,
    pub(in crate::app) home_albums: HomeAlbums,
    pub(in crate::app) daily_mix: DailyMixState,
    pub(in crate::app) auto_continue: AutoContinueState,
    pub(in crate::app) radio: RadioState,
    pub(in crate::app) genres: GenreLibrary,
    pub(in crate::app) search: SearchState,
    pub(in crate::app) collapsed_sections: HashSet<String>,
    pub(in crate::app) artist_details: ArtistDetails,
    pub(in crate::app) album_details: AlbumDetails,
    pub(in crate::app) library_playback: LibraryPlayback,
    pub(in crate::app) playlists: PlaylistLibrary,
    pub(in crate::app) favorites: FavoritesState,
    pub(in crate::app) hovered_item: Option<String>,
    pub(in crate::app) queue: PlaybackQueue,
    pub(in crate::app) queue_index: Option<usize>,
    /// 右侧 Up Next / History 抽屉；不再用独立页面替换当前浏览上下文。
    pub(in crate::app) queue_drawer_open: bool,
    pub(in crate::app) queue_drawer_tab: QueueDrawerTab,
    pub(in crate::app) now_playing: Option<Song>,
    pub(in crate::app) now_playing_quality: Option<TranscodingQuality>,
    pub(in crate::app) now_playing_audio_quality: Option<crate::source::AudioQuality>,
    pub(in crate::app) lyrics: LyricsState,
    pub(in crate::app) covers: CoverCache,
    pub(in crate::app) status: String,
    pub(in crate::app) settings: SettingsUi,
    /// Scoped cursor, multi-selection, and fixed range anchor.
    pub(in crate::app) song_selection: SongSelection,
    /// “曲目信息”对话框当前展示的歌曲。
    pub(in crate::app) song_info: Option<Song>,
    pub(in crate::app) view_before_now_playing: View,
    pub(in crate::app) playback_modes: PlayModes,
    pub(in crate::app) shuffle_history: ShuffleHistory,
    pub(in crate::app) occurrence: PlaybackOccurrence,
    pub(in crate::app) output_devices: OutputDevices,
    /// 本地收听事件和 History 列表；与播放会话一样按服务器隔离。
    pub(in crate::app) listening: ListeningState,
    /// 本地测量响度、所属服务器和播放增益选择。
    pub(in crate::app) loudness: LoudnessCache,
}

impl AppState {
    /// Replace page-owned data while retaining the playback occurrence, queue,
    /// lyrics, history, output state and source-qualified artwork requests.
    pub(in crate::app) fn reset_browsing(&mut self) {
        self.connection = ServerConnection::default();
        self.view = View::Home;
        self.artists = ArtistLibrary::default();
        self.albums = PagedCollection::default();
        self.home_albums = HomeAlbums::default();
        self.daily_mix = DailyMixState::default();
        self.genres = GenreLibrary::default();
        self.search = SearchState::default();
        self.collapsed_sections = HashSet::from(["random".to_string()]);
        self.artist_details = ArtistDetails::default();
        self.album_details = AlbumDetails::default();
        self.library_playback = LibraryPlayback::default();
        self.playlists = PlaylistLibrary::default();
        self.favorites = FavoritesState::default();
        self.hovered_item = None;
        self.song_info = None;
        self.view_before_now_playing = View::Home;
        self.song_selection.retain_playback_scopes();
    }
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
            now_playing_audio_quality: None,
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

#[cfg(test)]
mod tests;
