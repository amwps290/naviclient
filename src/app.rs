use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, ease_out_quint, hsla, img, linear_color_stop, linear_gradient, percentage, point, px,
    rems, Animation, AnimationExt, AppContext, Context, Entity, FontWeight, Hsla,
    Image as GpuiImage, ImageFormat as GpuiImageFormat, InteractiveElement, IntoElement,
    ParentElement, Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Transformation, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    v_flex, ActiveTheme, Icon, Selectable, Sizable, Theme, ThemeMode, TitleBar,
};
use rand::{seq::SliceRandom, thread_rng};
use smol::Timer;
use tokio::runtime::Runtime;

use crate::api::{format_duration, Api};
use crate::assets::{AppIcon, PlayerIcon};
use crate::audio::{format_playback, AudioHandle};
use crate::config;
use crate::models::{
    Album, Artist, Config, FavoriteKey, FavoriteKind, Favorites, Lyrics, Playlist, SearchResults,
    ServerInfo, Song, ThemePreference,
};
use crate::msg::{error_message, Msg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Home,
    Favorites,
    Artists,
    Albums,
    Playlists,
    Search,
    ArtistDetail,
    AlbumDetail,
    PlaylistDetail,
    NowPlaying,
    Queue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum PlaybackMode {
    #[default]
    RepeatAll,
    RepeatOne,
    Shuffle,
}

impl PlaybackMode {
    fn next(self) -> Self {
        match self {
            Self::RepeatAll => Self::RepeatOne,
            Self::RepeatOne => Self::Shuffle,
            Self::Shuffle => Self::RepeatAll,
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::RepeatAll => "Repeat all",
            Self::RepeatOne => "Repeat current track",
            Self::Shuffle => "Shuffle queue",
        }
    }
}

struct AppState {
    server: Option<ServerInfo>,
    view: View,
    loading: bool,
    artists: Vec<Artist>,
    albums: Vec<Album>,
    current_artist: Option<Artist>,
    artist_albums: Vec<Album>,
    current_album: Option<Album>,
    current_songs: Vec<Song>,
    playlists: Vec<Playlist>,
    favorites: Favorites,
    favorite_ids: HashSet<FavoriteKey>,
    pending_favorites: HashSet<FavoriteKey>,
    current_playlist: Option<Playlist>,
    playlist_songs: Vec<Song>,
    search_results: Option<SearchResults>,
    queue: Vec<Song>,
    queue_index: Option<usize>,
    now_playing: Option<Song>,
    lyrics: Option<Lyrics>,
    lyrics_song_id: Option<String>,
    lyrics_loading: bool,
    lyrics_error: Option<String>,
    covers: HashMap<String, Arc<GpuiImage>>,
    cover_palettes: HashMap<String, (Hsla, Hsla)>,
    requested_covers: HashSet<String>,
    status: String,
    error: Option<String>,
    settings_open: bool,
    view_before_now_playing: View,
    playback_mode: PlaybackMode,
    ended_handled: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            server: None,
            view: View::Home,
            loading: false,
            artists: Vec::new(),
            albums: Vec::new(),
            current_artist: None,
            artist_albums: Vec::new(),
            current_album: None,
            current_songs: Vec::new(),
            playlists: Vec::new(),
            favorites: Favorites::default(),
            favorite_ids: HashSet::new(),
            pending_favorites: HashSet::new(),
            current_playlist: None,
            playlist_songs: Vec::new(),
            search_results: None,
            queue: Vec::new(),
            queue_index: None,
            now_playing: None,
            lyrics: None,
            lyrics_song_id: None,
            lyrics_loading: false,
            lyrics_error: None,
            covers: HashMap::new(),
            cover_palettes: HashMap::new(),
            requested_covers: HashSet::new(),
            status: "Not connected".to_string(),
            error: None,
            settings_open: false,
            view_before_now_playing: View::Home,
            playback_mode: PlaybackMode::default(),
            ended_handled: false,
        }
    }
}

pub struct NavidromeApp {
    runtime: Runtime,
    api: Option<Api>,
    config: Config,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    audio: AudioHandle,
    default_cover: Arc<GpuiImage>,
    state: AppState,
    search_input: Entity<InputState>,
    server_input: Entity<InputState>,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    playback_slider: Entity<SliderState>,
    lyrics_scroll_handle: ScrollHandle,
    lyrics_scroll_target: Option<Pixels>,
    active_lyric_index: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl NavidromeApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = config::load();
        let api = Api::new(&config.server_url, &config.username, &config.password).ok();
        let (tx, rx) = mpsc::channel();
        let runtime = Runtime::new().expect("failed to create Tokio runtime");
        let audio = AudioHandle::start().expect("failed to start audio worker");
        let default_cover = Arc::new(GpuiImage::from_bytes(
            GpuiImageFormat::Png,
            include_bytes!("../assets/default-cover.png").to_vec(),
        ));
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search songs, albums, and artists")
                .clean_on_escape()
        });
        let server_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://localhost:4533")
                .default_value(config.server_url.clone())
        });
        let username_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Username")
                .default_value(config.username.clone())
        });
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .default_value(config.password.clone())
                .masked(true)
        });
        let playback_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(0.1)
                .default_value(0.0)
        });

        let mut app = Self {
            runtime,
            api,
            config,
            tx,
            rx,
            audio,
            default_cover,
            state: AppState::default(),
            search_input: search_input.clone(),
            server_input,
            username_input,
            password_input,
            playback_slider: playback_slider.clone(),
            lyrics_scroll_handle: ScrollHandle::new(),
            lyrics_scroll_target: None,
            active_lyric_index: None,
            _subscriptions: Vec::new(),
        };
        app.audio.set_volume(1.0);
        app._subscriptions.push(
            cx.subscribe(&search_input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_search(cx);
                    cx.notify();
                }
            }),
        );
        app._subscriptions.push(cx.subscribe(
            &playback_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                let duration = this.audio.state().duration.unwrap_or_default();
                if !duration.is_zero() {
                    this.audio
                        .seek(duration.mul_f32((*value / 100.0).clamp(0.0, 1.0)));
                }
                cx.notify();
            },
        ));
        app._subscriptions
            .push(cx.observe_window_appearance(window, |this, window, cx| {
                if this.config.theme == ThemePreference::System {
                    Theme::sync_system_appearance(Some(window), cx);
                    cx.notify();
                }
            }));

        if app.api.is_some() {
            app.refresh_library();
        } else {
            app.state.status = "Open Settings to configure the server".to_string();
        }
        if app.config.username.trim().is_empty() {
            app.state.status = "Configure your Navidrome server".to_string();
            app.state.settings_open = true;
        }

        cx.spawn(async move |this, cx| loop {
            Timer::after(Duration::from_millis(40)).await;
            if this
                .update(cx, |this, cx| {
                    this.poll_messages();
                    this.handle_playback_end();
                    this.update_active_lyric();
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
        app
    }

    fn spawn_future<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(future);
    }

    fn refresh_library(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        self.state.status = "Connecting to server...".to_string();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Ping(api.ping().await.map_err(error_message)));
        });
        self.load_artists();
        self.load_albums();
        self.load_playlists();
        self.load_favorites();
    }

    fn load_artists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Artists(api.artists().await.map_err(error_message)));
        });
    }

    fn load_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Albums(api.albums(200).await.map_err(error_message)));
        });
    }

    fn load_playlists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Playlists(api.playlists().await.map_err(error_message)));
        });
    }

    fn load_favorites(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Favorites(api.favorites().await.map_err(error_message)));
        });
    }

    fn open_artist(&mut self, artist: Artist) {
        self.state.current_artist = Some(artist.clone());
        self.state.artist_albums.clear();
        self.state.view = View::ArtistDetail;
        self.ensure_cover(artist.cover_art.as_deref());
        let Some(api) = self.api.clone() else { return };
        let artist_id = artist.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api.artist_albums(&artist_id).await.map_err(error_message);
            let _ = tx.send(Msg::ArtistAlbums { artist_id, result });
        });
    }

    fn open_album(&mut self, album: Album) {
        self.state.current_album = Some(album.clone());
        self.state.current_songs.clear();
        self.state.view = View::AlbumDetail;
        self.ensure_cover(album.cover_art.as_deref());
        let Some(api) = self.api.clone() else { return };
        let album_id = album.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api.album_songs(&album_id).await.map_err(error_message);
            let _ = tx.send(Msg::AlbumSongs { album_id, result });
        });
    }

    fn open_playlist(&mut self, playlist: Playlist) {
        self.state.current_playlist = Some(playlist.clone());
        self.state.playlist_songs.clear();
        self.state.view = View::PlaylistDetail;
        self.ensure_cover(playlist.cover_art.as_deref());
        let Some(api) = self.api.clone() else { return };
        let playlist_id = playlist.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api
                .playlist_songs(&playlist_id)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::PlaylistSongs {
                playlist_id,
                result,
            });
        });
    }

    fn submit_search(&mut self, cx: &Context<Self>) {
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() {
            self.state.search_results = None;
            return;
        }
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.search(&query, 120, 80, 60).await.map_err(error_message);
            let _ = tx.send(Msg::Search(result));
        });
    }

    fn toggle_favorite(&mut self, key: FavoriteKey) {
        let Some(api) = self.api.clone() else { return };
        if !self.state.pending_favorites.insert(key.clone()) {
            return;
        }

        let starred = !self.state.favorite_ids.contains(&key);
        if starred {
            self.state.favorite_ids.insert(key.clone());
        } else {
            self.state.favorite_ids.remove(&key);
        }

        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.set_favorite(&key, starred).await.map_err(error_message);
            let _ = tx.send(Msg::FavoriteChanged {
                key,
                starred,
                result,
            });
        });
    }

    fn ensure_cover(&mut self, cover_id: Option<&str>) {
        let Some(id) = cover_id else { return };
        if !self.state.requested_covers.insert(id.to_string()) {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let url = match api.cover_url(id, 500) {
            Ok(url) => url,
            Err(error) => {
                self.state.error = Some(format!("{error:#}"));
                return;
            }
        };
        let id = id.to_string();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.get_bytes(&url).await.map_err(error_message);
            let _ = tx.send(Msg::Cover { id, result });
        });
    }

    fn preload_covers(&mut self, ids: Vec<String>) {
        for id in ids {
            self.ensure_cover(Some(&id));
        }
    }

    fn play_song_list(&mut self, songs: &[Song], index: usize) {
        self.state.queue = songs.to_vec();
        self.play_queue_index(index);
    }

    fn load_lyrics(&mut self, song: &Song) {
        self.active_lyric_index = None;
        if self.state.lyrics_song_id.as_deref() == Some(song.id.as_str())
            && (self.state.lyrics.is_some() || self.state.lyrics_loading)
        {
            return;
        }

        self.state.lyrics_song_id = Some(song.id.clone());
        self.state.lyrics = None;
        self.state.lyrics_error = None;
        let Some(api) = self.api.clone() else {
            self.state.lyrics_loading = false;
            self.state.lyrics_error = Some("Configure a server to load lyrics".to_string());
            return;
        };

        self.state.lyrics_loading = true;
        let song = song.clone();
        let song_id = song.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.lyrics(&song).await.map_err(error_message);
            let _ = tx.send(Msg::Lyrics { song_id, result });
        });
    }

    fn play_queue_index(&mut self, index: usize) {
        let Some(song) = self.state.queue.get(index).cloned() else {
            return;
        };
        self.state.queue_index = Some(index);
        self.state.now_playing = Some(song.clone());
        self.state.ended_handled = false;
        self.ensure_cover(song.cover_art.as_deref());
        self.load_lyrics(&song);
        let Some(api) = self.api.clone() else {
            self.state.error = Some("Configure a server before playing".to_string());
            return;
        };
        match api.stream_url(&song.id) {
            Ok(url) => {
                let duration = song
                    .duration
                    .and_then(|seconds| u64::try_from(seconds).ok())
                    .map(Duration::from_secs);
                self.audio.play(url, duration);
            }
            Err(error) => self.state.error = Some(format!("{error:#}")),
        }
    }

    fn random_queue_index(&self, excluding: Option<usize>) -> Option<usize> {
        let mut indices = (0..self.state.queue.len()).collect::<Vec<_>>();
        if let Some(excluding) = excluding {
            indices.retain(|index| *index != excluding);
        }
        indices.choose(&mut thread_rng()).copied().or(excluding)
    }

    fn advance_queue(&mut self, forward: bool) {
        let Some(index) = self.state.queue_index else {
            return;
        };
        let len = self.state.queue.len();
        if len == 0 {
            return;
        }

        let next = if forward {
            match self.state.playback_mode {
                PlaybackMode::Shuffle => self.random_queue_index(Some(index)).unwrap_or(index),
                _ => (index + 1) % len,
            }
        } else {
            (index + len - 1) % len
        };

        self.play_queue_index(next);
    }

    fn cycle_playback_mode(&mut self) {
        self.state.playback_mode = self.state.playback_mode.next();
    }

    fn skip(&mut self, offset: i32) {
        if offset > 0 {
            self.advance_queue(true);
        } else if offset < 0 {
            self.advance_queue(false);
        }
    }

    fn toggle_playback(&mut self) {
        let playback = self.audio.state();
        if playback.active {
            if playback.paused {
                self.audio.resume()
            } else {
                self.audio.pause()
            }
        } else if let Some(index) = self.state.queue_index {
            self.play_queue_index(index);
        }
    }

    fn open_now_playing(&mut self) {
        if self.state.view != View::NowPlaying {
            self.state.view_before_now_playing = self.state.view;
        }
        self.state.settings_open = false;
        self.state.view = View::NowPlaying;
        self.active_lyric_index = None;
        self.lyrics_scroll_target = None;
    }

    fn leave_now_playing(&mut self) {
        self.state.view = self.state.view_before_now_playing;
        self.active_lyric_index = None;
        self.lyrics_scroll_target = None;
    }

    fn save_settings(&mut self, cx: &Context<Self>) {
        let new_config = Config {
            server_url: self.server_input.read(cx).value().trim().to_string(),
            username: self.username_input.read(cx).value().trim().to_string(),
            password: self.password_input.read(cx).value().to_string(),
            theme: self.config.theme,
        };
        self.config = new_config;
        if let Err(error) = config::save(&self.config) {
            self.state.status = format!("Config save failed: {error:#}");
        }
        self.audio.stop();
        self.state = AppState::default();
        match Api::new(
            &self.config.server_url,
            &self.config.username,
            &self.config.password,
        ) {
            Ok(api) => {
                self.api = Some(api);
                self.refresh_library();
            }
            Err(error) => {
                self.api = None;
                self.state.status = "Invalid server URL".to_string();
                self.state.error = Some(format!("{error:#}"));
                self.state.settings_open = true;
            }
        }
    }

    fn set_theme(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.theme = preference;
        match preference {
            ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
            ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
        }
        if let Err(error) = config::save(&self.config) {
            self.state.status = format!("Theme save failed: {error:#}");
        }
        cx.notify();
    }

    fn poll_messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Msg::Ping(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(info) => {
                            let server_name = info.app_name.as_deref().unwrap_or("Navidrome");
                            self.state.status = format!(
                                "{server_name} {} - {}",
                                info.version,
                                self.api
                                    .as_ref()
                                    .map(|api| api.base_url().to_string())
                                    .unwrap_or_default()
                            );
                            self.state.server = Some(info);
                        }
                        Err(error) => {
                            self.state.status = "Connection failed".to_string();
                            self.state.error = Some(error);
                        }
                    }
                }
                Msg::Artists(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(artists) => {
                            let covers = artists
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(40)
                                .collect();
                            self.state.artists = artists;
                            self.preload_covers(covers);
                        }
                        Err(error) => self.state.error = Some(error),
                    }
                }
                Msg::ArtistAlbums { artist_id, result } => {
                    self.state.loading = false;
                    if self.state.current_artist.as_ref().map(|a| a.id.as_str())
                        == Some(artist_id.as_str())
                    {
                        match result {
                            Ok(albums) => {
                                let covers = albums
                                    .iter()
                                    .filter_map(|item| item.cover_art.clone())
                                    .collect();
                                self.state.artist_albums = albums;
                                self.preload_covers(covers);
                            }
                            Err(error) => self.state.error = Some(error),
                        }
                    }
                }
                Msg::Albums(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(mut albums) => {
                            albums.sort_by_key(|album| album.created.clone().unwrap_or_default());
                            albums.reverse();
                            let covers = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(80)
                                .collect();
                            self.state.albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => self.state.error = Some(error),
                    }
                }
                Msg::AlbumSongs { album_id, result } => {
                    self.state.loading = false;
                    if self.state.current_album.as_ref().map(|a| a.id.as_str())
                        == Some(album_id.as_str())
                    {
                        match result {
                            Ok(mut songs) => {
                                songs.sort_by_key(|song| song.track.unwrap_or(i32::MAX));
                                let covers = songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone())
                                    .collect();
                                self.state.current_songs = songs;
                                self.preload_covers(covers);
                            }
                            Err(error) => self.state.error = Some(error),
                        }
                    }
                }
                Msg::Playlists(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(playlists) => self.state.playlists = playlists,
                        Err(error) => self.state.error = Some(error),
                    }
                }
                Msg::Favorites(result) => match result {
                    Ok(favorites) => {
                        let mut favorite_ids = HashSet::new();
                        favorite_ids.extend(
                            favorites
                                .artists
                                .iter()
                                .map(|artist| FavoriteKey::new(FavoriteKind::Artist, &artist.id)),
                        );
                        favorite_ids.extend(
                            favorites
                                .albums
                                .iter()
                                .map(|album| FavoriteKey::new(FavoriteKind::Album, &album.id)),
                        );
                        favorite_ids.extend(
                            favorites
                                .songs
                                .iter()
                                .map(|song| FavoriteKey::new(FavoriteKind::Song, &song.id)),
                        );
                        for key in &self.state.pending_favorites {
                            if self.state.favorite_ids.contains(key) {
                                favorite_ids.insert(key.clone());
                            } else {
                                favorite_ids.remove(key);
                            }
                        }
                        let covers = favorites
                            .albums
                            .iter()
                            .filter_map(|album| album.cover_art.clone())
                            .chain(
                                favorites
                                    .artists
                                    .iter()
                                    .filter_map(|artist| artist.cover_art.clone()),
                            )
                            .chain(
                                favorites
                                    .songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone()),
                            )
                            .collect();
                        self.state.favorites = favorites;
                        self.state.favorite_ids = favorite_ids;
                        self.preload_covers(covers);
                    }
                    Err(error) => self.state.error = Some(error),
                },
                Msg::FavoriteChanged {
                    key,
                    starred,
                    result,
                } => {
                    self.state.pending_favorites.remove(&key);
                    if let Err(error) = result {
                        if starred {
                            self.state.favorite_ids.remove(&key);
                        } else {
                            self.state.favorite_ids.insert(key);
                        }
                        self.state.error = Some(error);
                    } else {
                        self.load_favorites();
                    }
                }
                Msg::PlaylistSongs {
                    playlist_id,
                    result,
                } => {
                    self.state.loading = false;
                    if self.state.current_playlist.as_ref().map(|p| p.id.as_str())
                        == Some(playlist_id.as_str())
                    {
                        match result {
                            Ok(songs) => {
                                let covers = songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone())
                                    .collect();
                                self.state.playlist_songs = songs;
                                self.preload_covers(covers);
                            }
                            Err(error) => self.state.error = Some(error),
                        }
                    }
                }
                Msg::Search(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(results) => self.state.search_results = Some(results),
                        Err(error) => self.state.error = Some(error),
                    }
                }
                Msg::Lyrics { song_id, result } => {
                    if self.state.now_playing.as_ref().map(|song| song.id.as_str())
                        == Some(song_id.as_str())
                    {
                        self.state.lyrics_loading = false;
                        self.active_lyric_index = None;
                        match result {
                            Ok(lyrics) => {
                                self.state.lyrics = Some(lyrics);
                                self.state.lyrics_error = None;
                            }
                            Err(error) => {
                                self.state.lyrics = None;
                                self.state.lyrics_error = Some(error);
                            }
                        }
                    }
                }
                Msg::Cover { id, result } => {
                    if let Ok(bytes) = result {
                        if let Some(palette) = extract_cover_palette(&bytes) {
                            self.state.cover_palettes.insert(id.clone(), palette);
                        }
                        if let Ok(format) = image::guess_format(&bytes) {
                            if let Some(format) = gpui_image_format(format) {
                                self.state
                                    .covers
                                    .insert(id, Arc::new(GpuiImage::from_bytes(format, bytes)));
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_playback_end(&mut self) {
        let playback = self.audio.state();
        if self.state.ended_handled || self.state.now_playing.is_none() {
            return;
        }
        if let Some(error) = playback.error {
            self.state.ended_handled = true;
            self.state.error = Some(error);
        } else if playback.ended {
            self.state.ended_handled = true;
            match self.state.playback_mode {
                PlaybackMode::RepeatAll => self.advance_queue(true),
                PlaybackMode::RepeatOne => {
                    if let Some(index) = self.state.queue_index {
                        self.play_queue_index(index);
                    }
                }
                PlaybackMode::Shuffle => {
                    if let Some(index) = self.random_queue_index(self.state.queue_index) {
                        self.play_queue_index(index);
                    }
                }
            }
        }
    }

    fn update_active_lyric(&mut self) {
        if self.state.view != View::NowPlaying {
            self.lyrics_scroll_target = None;
            return;
        }

        let active = self
            .state
            .lyrics
            .as_ref()
            .filter(|lyrics| lyrics.is_synced())
            .and_then(|lyrics| lyrics.active_line_index(self.audio.state().position));

        if active != self.active_lyric_index {
            self.active_lyric_index = active;
            if let Some(index) = active {
                let item_index = index + 1;
                if let Some(item_bounds) = self.lyrics_scroll_handle.bounds_for_item(item_index) {
                    let viewport_bounds = self.lyrics_scroll_handle.bounds();
                    self.lyrics_scroll_target =
                        Some(viewport_bounds.center().y - item_bounds.center().y);
                } else {
                    self.lyrics_scroll_handle.scroll_to_item(item_index);
                }
            }
        }

        if let Some(target) = self.lyrics_scroll_target {
            let current = self.lyrics_scroll_handle.offset();
            let distance = f32::from(target - current.y);
            if distance.abs() <= 0.75 {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, target));
                self.lyrics_scroll_target = None;
            } else {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, current.y + px(distance * 0.24)));
            }
        }
    }

    fn nav_button(&self, view: View, label: &'static str, cx: &Context<Self>) -> Button {
        let selected = self.state.view == view
            || matches!(
                (view, self.state.view),
                (View::Artists, View::ArtistDetail)
                    | (View::Albums, View::AlbumDetail)
                    | (View::Playlists, View::PlaylistDetail)
            );
        let button = Button::new(SharedString::from(format!("nav-{label}")))
            .label(label)
            .w_full()
            .justify_start()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.state.settings_open = false;
                this.state.view = view;
                match view {
                    View::Artists if this.state.artists.is_empty() => this.load_artists(),
                    View::Albums if this.state.albums.is_empty() => this.load_albums(),
                    View::Playlists if this.state.playlists.is_empty() => this.load_playlists(),
                    View::Favorites => this.load_favorites(),
                    _ => {}
                }
                cx.notify();
            }));
        button.ghost().selected(selected)
    }

    fn favorite_button(&self, kind: FavoriteKind, id: &str, cx: &Context<Self>) -> Button {
        let key = FavoriteKey::new(kind, id);
        let starred = self.state.favorite_ids.contains(&key);
        let pending = self.state.pending_favorites.contains(&key);
        let kind_name = match kind {
            FavoriteKind::Artist => "artist",
            FavoriteKind::Album => "album",
            FavoriteKind::Song => "song",
        };

        Button::new(SharedString::from(format!("favorite-{kind_name}-{id}")))
            .icon(if starred {
                AppIcon::HeartFilled
            } else {
                AppIcon::Heart
            })
            .tooltip(if starred {
                "Remove from favorites"
            } else {
                "Add to favorites"
            })
            .ghost()
            .small()
            .opacity(if pending { 0.6 } else { 1.0 })
            .rounded_full()
            .text_color(if starred {
                cx.theme().red
            } else {
                cx.theme().muted_foreground
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_favorite(key.clone());
                cx.notify();
            }))
    }

    fn playback_mode_button(&self, cx: &Context<Self>) -> Button {
        let button = Button::new("player-playback-mode")
            .tooltip(self.state.playback_mode.tooltip())
            .ghost()
            .with_size(px(30.0))
            .rounded_full()
            .on_click(cx.listener(|this, _, _, cx| {
                this.cycle_playback_mode();
                cx.notify();
            }));

        match self.state.playback_mode {
            PlaybackMode::RepeatAll => button.icon(AppIcon::Repeat),
            PlaybackMode::RepeatOne => button.icon(AppIcon::RepeatOne),
            PlaybackMode::Shuffle => button.icon(AppIcon::Shuffle),
        }
    }

    fn render_cover(
        &self,
        cover_id: Option<&str>,
        size: f32,
        _cx: &Context<Self>,
    ) -> gpui::AnyElement {
        if let Some(image) = cover_id.and_then(|id| self.state.covers.get(id)) {
            return img(image.clone())
                .w(px(size))
                .h(px(size))
                .rounded_lg()
                .into_any_element();
        }
        img(self.default_cover.clone())
            .w(px(size))
            .h(px(size))
            .flex_shrink_0()
            .rounded_lg()
            .into_any_element()
    }

    fn render_vinyl_record(
        &self,
        cover_id: Option<&str>,
        size: f32,
        rotation_phase: f32,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let cover = cover_id
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        let label_size = size * 0.42;
        let label_offset = (size - label_size) * 0.5;
        let inner_cover_size = label_size - 10.0;
        let highlight = Icon::new(AppIcon::VinylHighlight)
            .absolute()
            .top_0()
            .left_0()
            .with_size(px(size))
            .text_color(hsla(0.0, 0.0, 1.0, 0.24))
            .transform(Transformation::rotate(percentage(rotation_phase)));

        div()
            .relative()
            .w(px(size))
            .h(px(size))
            .flex_none()
            .rounded_full()
            .bg(hsla(0.0, 0.0, 0.07, 1.0))
            .shadow_md()
            .child(
                div()
                    .absolute()
                    .top(px(size * 0.07))
                    .left(px(size * 0.07))
                    .size(px(size * 0.86))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.08)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(size * 0.14))
                    .left(px(size * 0.14))
                    .size(px(size * 0.72))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.06)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(size * 0.21))
                    .left(px(size * 0.21))
                    .size(px(size * 0.58))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.05)),
            )
            .child(highlight)
            .child(
                div()
                    .absolute()
                    .top(px(label_offset))
                    .left(px(label_offset))
                    .size(px(label_size))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.14))
                    .bg(hsla(0.0, 0.0, 0.08, 1.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(img(cover).size(px(inner_cover_size)).rounded_full())
                    .child(
                        div()
                            .absolute()
                            .size(px(9.0))
                            .rounded_full()
                            .bg(hsla(0.0, 0.0, 0.08, 1.0)),
                    ),
            )
            .child(
                Icon::new(AppIcon::Tonearm)
                    .absolute()
                    .top_0()
                    .left_0()
                    .with_size(px(size))
                    .text_color(cx.theme().foreground.opacity(0.5)),
            )
            .into_any_element()
    }

    fn page_header(
        &self,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.into()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle.into()),
            )
            .into_any_element()
    }

    fn render_scrolling_title(
        &self,
        text: SharedString,
        viewport_width: f32,
        id: SharedString,
        window: &mut Window,
    ) -> gpui::AnyElement {
        let font_size = rems(0.875).to_pixels(window.rem_size());
        let text_style = window.text_style().highlight(FontWeight::MEDIUM);
        let text_width = window
            .text_system()
            .shape_line(
                text.clone(),
                font_size,
                &[text_style.to_run(text.len())],
                None,
            )
            .width;
        let viewport_width = px(viewport_width);

        if text_width <= viewport_width {
            return div()
                .w(viewport_width)
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(text)
                .into_any_element();
        }

        let gap = px(32.0);
        let travel = text_width + gap;
        let travel_px = f32::from(travel);
        let duration_seconds = (travel_px / 28.0_f32 + 1.5_f32).max(4.0_f32);
        let hold_fraction = (1.5_f32 / duration_seconds).clamp(0.0_f32, 0.8_f32);
        let animation = Animation::new(Duration::from_secs_f32(duration_seconds))
            .repeat()
            .with_easing(move |delta| {
                if delta <= hold_fraction {
                    0.0
                } else {
                    (delta - hold_fraction) / (1.0 - hold_fraction)
                }
            });
        let repeated_text = text.clone();

        div()
            .w(viewport_width)
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(
                h_flex()
                    .flex_none()
                    .gap(gap)
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(text)
                    .child(repeated_text)
                    .with_animation(id, animation, move |this, delta| {
                        this.ml(px(-travel_px * delta))
                    }),
            )
            .into_any_element()
    }

    fn render_album_grid(
        &self,
        albums: &[Album],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui::AnyElement {
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children(albums.iter().map(|album| {
                let album_for_click = album.clone();
                v_flex()
                    .w(px(176.0))
                    .gap_2()
                    .child(
                        div()
                            .relative()
                            .w(px(176.0))
                            .h(px(176.0))
                            .child(self.render_cover(album.cover_art.as_deref(), 176.0, cx))
                            .child(
                                div()
                                    .absolute()
                                    .top_2()
                                    .right_2()
                                    .child(self.favorite_button(
                                        FavoriteKind::Album,
                                        &album.id,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("album-{}", album.id)))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .child(self.render_scrolling_title(
                                album.name.clone().into(),
                                144.0,
                                SharedString::from(format!("album-title-{}", album.id)),
                                window,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_album(album_for_click.clone());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(format!(
                                "{}{}",
                                album.artist,
                                album
                                    .year
                                    .map(|year| format!(" - {year}"))
                                    .unwrap_or_default()
                            )),
                    )
            }))
            .into_any_element()
    }

    fn render_artist_grid(
        &self,
        artists: &[Artist],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui::AnyElement {
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children(artists.iter().map(|artist| {
                let artist_for_click = artist.clone();
                v_flex()
                    .w(px(152.0))
                    .gap_2()
                    .child(
                        div()
                            .relative()
                            .w(px(152.0))
                            .h(px(152.0))
                            .child(self.render_cover(artist.cover_art.as_deref(), 152.0, cx))
                            .child(
                                div()
                                    .absolute()
                                    .top_2()
                                    .right_2()
                                    .child(self.favorite_button(
                                        FavoriteKind::Artist,
                                        &artist.id,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("artist-{}", artist.id)))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .child(self.render_scrolling_title(
                                artist.name.clone().into(),
                                120.0,
                                SharedString::from(format!("artist-title-{}", artist.id)),
                                window,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_artist(artist_for_click.clone());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                artist
                                    .album_count
                                    .map(|count| format!("{count} albums"))
                                    .unwrap_or_else(|| "Artist".to_string()),
                            ),
                    )
            }))
            .into_any_element()
    }

    fn render_playing_indicator(
        &self,
        song_id: &str,
        current: bool,
        animated: bool,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        if !current {
            return div().w(px(16.0)).h(px(16.0)).flex_none().into_any_element();
        }

        let bar_heights = [7.0_f32, 13.0_f32, 9.0_f32];
        h_flex()
            .w(px(16.0))
            .h(px(16.0))
            .flex_none()
            .items_end()
            .justify_center()
            .gap(px(2.0))
            .children(bar_heights.into_iter().enumerate().map(|(index, height)| {
                let bar = div()
                    .w(px(3.0))
                    .h(px(height))
                    .rounded_full()
                    .bg(cx.theme().info);

                if animated {
                    let phase = index as f32 * 0.23;
                    bar.with_animation(
                        SharedString::from(format!("playing-{song_id}-{index}")),
                        Animation::new(Duration::from_millis(720))
                            .repeat()
                            .with_easing(move |delta| {
                                let shifted = (delta + phase) % 1.0;
                                1.0 - (shifted * 2.0 - 1.0).abs()
                            }),
                        |bar, delta| bar.h(px(4.0 + delta * 10.0)),
                    )
                    .into_any_element()
                } else {
                    bar.opacity(0.7).into_any_element()
                }
            }))
            .into_any_element()
    }

    fn render_song_list(&self, songs: &[Song], cx: &Context<Self>) -> gpui::AnyElement {
        let queue_source = songs.to_vec();
        let playback = self.audio.state();
        v_flex()
            .w_full()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .overflow_hidden()
            .child(
                h_flex()
                    .h(px(36.0))
                    .px_3()
                    .gap_3()
                    .bg(cx.theme().secondary)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(34.0)))
                    .child(div().w(px(42.0)))
                    .child(div().flex_1().child("Title"))
                    .child(div().w(px(180.0)).child("Artist"))
                    .child(div().w(px(64.0)).text_right().child("Time")),
            )
            .children(songs.iter().enumerate().map(|(index, song)| {
                let queue = queue_source.clone();
                let current = self
                    .state
                    .now_playing
                    .as_ref()
                    .is_some_and(|playing| playing.id == song.id);
                let animated = current && playback.active && !playback.paused;
                let album = if song.album.trim().is_empty() {
                    "Unknown album".to_string()
                } else {
                    song.album.clone()
                };
                let row = h_flex()
                    .id(SharedString::from(format!("song-row-{}", song.id)))
                    .h(px(60.0))
                    .px_3()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border.opacity(0.6))
                    .hover(|style| style.bg(cx.theme().accent.opacity(0.12)))
                    .cursor_pointer();
                let row = if current {
                    row.bg(cx.theme().info.opacity(0.1))
                } else {
                    row
                };

                row.child(self.favorite_button(FavoriteKind::Song, &song.id, cx))
                    .child(
                        div()
                            .w(px(42.0))
                            .h(px(42.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border.opacity(0.7))
                            .bg(cx.theme().background)
                            .child(self.render_cover(song.cover_art.as_deref(), 36.0, cx)),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .text_color(if current {
                                cx.theme().info
                            } else {
                                cx.theme().foreground
                            })
                            .child(self.render_playing_indicator(&song.id, current, animated, cx))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .line_height(rems(1.25))
                                            .truncate()
                                            .child(song.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .line_height(rems(1.2))
                                            .truncate()
                                            .child(album),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .w(px(180.0))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(song.artist.clone()),
                    )
                    .child(
                        div()
                            .w(px(64.0))
                            .text_sm()
                            .text_right()
                            .text_color(cx.theme().muted_foreground)
                            .child(format_duration(song.duration)),
                    )
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        if event.standard_click() && event.click_count() == 2 {
                            this.play_song_list(&queue, index);
                            cx.notify();
                        }
                    }))
            }))
            .into_any_element()
    }

    fn render_server_status_card(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let status_label = if self.state.loading {
            "Connecting..."
        } else if self.state.server.is_some() {
            "Connected"
        } else {
            "Not connected"
        };
        let status_color = if self.state.error.is_some() {
            cx.theme().red
        } else if self.state.server.is_some() {
            cx.theme().accent
        } else {
            cx.theme().muted_foreground
        };
        let server_name = self
            .state
            .server
            .as_ref()
            .map(|server| {
                format!(
                    "{} {}",
                    server.app_name.as_deref().unwrap_or("Navidrome"),
                    server.version
                )
            })
            .unwrap_or_else(|| "Server entry".to_string());
        let server_url = if self.config.server_url.trim().is_empty() {
            "No server URL configured".to_string()
        } else {
            self.config.server_url.clone()
        };
        let username = if self.config.username.trim().is_empty() {
            "No username configured".to_string()
        } else {
            self.config.username.clone()
        };

        v_flex()
            .w_full()
            .gap_4()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary.opacity(0.35))
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .gap_4()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Primary server"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(server_name),
                            )
                            .child(div().text_xs().text_color(status_color).child(status_label)),
                    )
                    .when(self.api.is_some(), |this| {
                        this.child(
                            Button::new("settings-refresh")
                                .icon(AppIcon::Refresh)
                                .tooltip("Refresh library")
                                .loading(self.state.loading)
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_library();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_6()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Server URL"),
                            )
                            .child(div().text_sm().truncate().child(server_url)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Username"),
                            )
                            .child(div().text_sm().truncate().child(username)),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.state.status.clone()),
            )
            .when_some(self.state.error.as_ref(), |this, error| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().red)
                        .child(error.clone()),
                )
            })
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Server URL"),
                    )
                    .child(Input::new(&self.server_input).w_full()),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Username"),
                    )
                    .child(Input::new(&self.username_input).w_full()),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Password"),
                    )
                    .child(Input::new(&self.password_input).w_full()),
            )
            .child(
                h_flex().justify_end().gap_2().child(
                    Button::new("save-settings")
                        .label("Save and connect")
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_settings(cx);
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_settings(&self, cx: &Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .max_w(px(840.0))
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Servers"),
                    )
                    .child(self.render_server_status_card(cx)),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Appearance"),
                    )
                    .child(
                        v_flex()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Theme"),
                            )
                            .child(
                                h_flex().gap_2().children(
                                    [
                                        ThemePreference::Light,
                                        ThemePreference::Dark,
                                        ThemePreference::System,
                                    ]
                                    .into_iter()
                                    .map(|preference| {
                                        Button::new(SharedString::from(format!(
                                            "theme-{}",
                                            preference.label().to_lowercase()
                                        )))
                                        .label(preference.label())
                                        .selected(self.config.theme == preference)
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.set_theme(preference, window, cx);
                                            }),
                                        )
                                    }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn error_banner(&self, cx: &Context<Self>) -> Option<gpui::AnyElement> {
        self.state.error.as_ref().map(|message| {
            div()
                .p_3()
                .rounded_md()
                .bg(cx.theme().red.opacity(0.12))
                .text_color(cx.theme().red)
                .child(message.clone())
                .into_any_element()
        })
    }

    fn render_now_playing(&self, window: &Window, cx: &Context<Self>) -> gpui::AnyElement {
        let Some(song) = &self.state.now_playing else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("No track playing"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Choose a song from your library."),
                )
                .into_any_element();
        };

        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        let max_cover_size = if viewport_width < 1_100.0 {
            300.0
        } else {
            380.0
        };
        let cover_size = (viewport_height - 300.0).clamp(220.0, max_cover_size);
        let playback = self.audio.state();
        let active_line = self
            .state
            .lyrics
            .as_ref()
            .and_then(|lyrics| lyrics.active_line_index(playback.position));
        let lyrics = self.state.lyrics.as_ref();
        let lyrics_body = if self.state.lyrics_loading {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Loading lyrics...")
                .into_any_element()
        } else if let Some(error) = &self.state.lyrics_error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child("Lyrics are unavailable for this track.")
                .child(div().text_xs().child(error.clone()))
                .into_any_element()
        } else if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.lines.is_empty()) {
            let synced = lyrics.is_synced();
            v_flex()
                .id("lyrics-scroll")
                .flex_1()
                .min_h_0()
                .px_6()
                .overflow_y_scroll()
                .track_scroll(&self.lyrics_scroll_handle)
                .child(div().h(px(240.0)).flex_none())
                .children(lyrics.lines.iter().enumerate().map(|(index, lyric_line)| {
                    let current = synced && active_line == Some(index);
                    let distance = active_line
                        .map(|active| active.abs_diff(index))
                        .unwrap_or(usize::MAX);
                    let text = if lyric_line.text.is_empty() {
                        " ".to_string()
                    } else {
                        lyric_line.text.clone()
                    };
                    let start_ms = lyric_line.start_ms;
                    let line = div()
                        .id(SharedString::from(format!(
                            "lyric-line-{}-{index}",
                            song.id
                        )))
                        .w_full()
                        .max_w(px(760.0))
                        .min_h(px(56.0))
                        .mx_auto()
                        .px_4()
                        .py_3()
                        .rounded_lg()
                        .text_center()
                        .line_height(rems(1.5))
                        .when_some(start_ms, |this, start_ms| {
                            this.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.audio.seek(Duration::from_millis(start_ms));
                                    cx.notify();
                                }))
                        })
                        .child(text);

                    if current {
                        line.text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .bg(cx.theme().info.opacity(0.12))
                            .with_animation(
                                SharedString::from(format!("active-lyric-{}-{index}", song.id)),
                                Animation::new(Duration::from_millis(220))
                                    .with_easing(ease_out_quint()),
                                |this, delta| this.opacity(0.72 + delta * 0.28),
                            )
                            .into_any_element()
                    } else if synced {
                        let opacity = match distance {
                            1 => 0.72,
                            2 => 0.56,
                            3 => 0.44,
                            _ => 0.32,
                        };
                        line.text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground.opacity(opacity))
                            .into_any_element()
                    } else {
                        line.text_lg()
                            .text_color(cx.theme().foreground.opacity(0.72))
                            .into_any_element()
                    }
                }))
                .child(div().h(px(280.0)).flex_none())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("No lyrics are available for this track.")
                .into_any_element()
        };

        let rotation_phase = vinyl_rotation_phase(playback.position);
        h_flex()
            .size_full()
            .min_h_0()
            .px_8()
            .py_5()
            .gap_8()
            .items_center()
            .child(
                v_flex()
                    .w(px((cover_size + 40.0).max(320.0)))
                    .flex_none()
                    .items_center()
                    .gap_4()
                    .child(self.render_vinyl_record(
                        song.cover_art.as_deref(),
                        cover_size,
                        rotation_phase,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(song.artist.clone()),
                            )
                            .when(!song.album.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(song.album.clone()),
                                )
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl_6()
                    .border_l_1()
                    .border_color(cx.theme().border.opacity(0.55))
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .pb_3()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Lyrics"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} - {}", song.title, song.artist)),
                            ),
                    )
                    .child(lyrics_body),
            )
            .into_any_element()
    }

    fn render_content(&self, window: &mut Window, cx: &Context<Self>) -> gpui::AnyElement {
        if self.state.settings_open {
            return self.render_settings(cx);
        }

        match self.state.view {
            View::Home => v_flex()
                .gap_5()
                .child(
                    self.page_header(
                        "Home",
                        self.state
                            .server
                            .as_ref()
                            .map(|info| format!("Connected to Navidrome API {}", info.version))
                            .unwrap_or_else(|| self.state.status.clone()),
                        cx,
                    ),
                )
                .children(self.error_banner(cx))
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Newest albums"),
                )
                .child(
                    self.render_album_grid(
                        &self
                            .state
                            .albums
                            .iter()
                            .take(30)
                            .cloned()
                            .collect::<Vec<_>>(),
                        cx,
                        window,
                    ),
                )
                .into_any_element(),
            View::Favorites => {
                let favorites = &self.state.favorites;
                let total =
                    favorites.artists.len() + favorites.albums.len() + favorites.songs.len();
                let mut content = v_flex()
                    .gap_5()
                    .child(self.page_header(
                        "Favorites",
                        format!("{total} starred items from your Navidrome library"),
                        cx,
                    ))
                    .children(self.error_banner(cx));

                if total == 0 {
                    content = content.child(
                        div()
                            .py_8()
                            .text_color(cx.theme().muted_foreground)
                            .child("Click a heart on a song, album, or artist to add it here."),
                    );
                }
                if !favorites.artists.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Artists"),
                        )
                        .child(self.render_artist_grid(&favorites.artists, cx, window));
                }
                if !favorites.albums.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Albums"),
                        )
                        .child(self.render_album_grid(&favorites.albums, cx, window));
                }
                if !favorites.songs.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Songs"),
                        )
                        .child(self.render_song_list(&favorites.songs, cx));
                }

                content.into_any_element()
            }
            View::Artists => v_flex()
                .gap_5()
                .child(self.page_header(
                    "Artists",
                    format!("{} artists in your library", self.state.artists.len()),
                    cx,
                ))
                .children(self.error_banner(cx))
                .child(self.render_artist_grid(&self.state.artists, cx, window))
                .into_any_element(),
            View::Albums => v_flex()
                .gap_5()
                .child(self.page_header(
                    "Albums",
                    format!("{} albums in your library", self.state.albums.len()),
                    cx,
                ))
                .children(self.error_banner(cx))
                .child(self.render_album_grid(&self.state.albums, cx, window))
                .into_any_element(),
            View::Playlists => {
                let playlist_list =
                    v_flex()
                        .w_full()
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded_lg()
                        .overflow_hidden()
                        .children(self.state.playlists.iter().enumerate().map(
                            |(index, playlist)| {
                                let playlist_for_click = playlist.clone();
                                let track_count = playlist.song_count.unwrap_or_default();
                                let details = playlist
                                    .owner
                                    .as_deref()
                                    .filter(|owner| !owner.trim().is_empty())
                                    .map(|owner| format!("{track_count} tracks | {owner}"))
                                    .unwrap_or_else(|| format!("{track_count} tracks"));

                                h_flex()
                                    .id(SharedString::from(format!("playlist-row-{}", playlist.id)))
                                    .h(px(68.0))
                                    .px_3()
                                    .gap_3()
                                    .items_center()
                                    .cursor_pointer()
                                    .when(index > 0, |this| {
                                        this.border_t_1().border_color(cx.theme().border)
                                    })
                                    .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                                    .child(self.render_cover(
                                        playlist.cover_art.as_deref(),
                                        48.0,
                                        cx,
                                    ))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .truncate()
                                                    .child(playlist.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .truncate()
                                                    .child(details),
                                            ),
                                    )
                                    .child(
                                        Icon::new(AppIcon::ChevronRight)
                                            .small()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_playlist(playlist_for_click.clone());
                                        cx.notify();
                                    }))
                            },
                        ));

                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        "Playlists",
                        format!("{} playlists", self.state.playlists.len()),
                        cx,
                    ))
                    .children(self.error_banner(cx))
                    .when(self.state.playlists.is_empty(), |this| {
                        this.child(
                            div()
                                .py_8()
                                .text_color(cx.theme().muted_foreground)
                                .child("No playlists are available."),
                        )
                    })
                    .when(!self.state.playlists.is_empty(), |this| {
                        this.child(playlist_list)
                    })
                    .into_any_element()
            }
            View::Search => {
                let mut content = v_flex()
                    .gap_5()
                    .child(self.page_header(
                        "Search",
                        "Find music across the entire server library.",
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Input::new(&self.search_input).flex_1())
                            .child(
                                Button::new("run-search")
                                    .label("Search")
                                    .primary()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_search(cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(self.error_banner(cx));
                if let Some(results) = &self.state.search_results {
                    if !results.artists.is_empty() {
                        content = content
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Artists"),
                            )
                            .child(self.render_artist_grid(&results.artists, cx, window));
                    }
                    if !results.albums.is_empty() {
                        content = content
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Albums"),
                            )
                            .child(self.render_album_grid(&results.albums, cx, window));
                    }
                    if !results.songs.is_empty() {
                        content = content
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Songs"),
                            )
                            .child(self.render_song_list(&results.songs, cx));
                    }
                }
                content.into_any_element()
            }
            View::ArtistDetail => {
                let Some(artist) = &self.state.current_artist else {
                    return div().child("Artist not found").into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        artist.name.clone(),
                        format!("{} albums", self.state.artist_albums.len()),
                        cx,
                    ))
                    .children(self.error_banner(cx))
                    .child(self.render_album_grid(&self.state.artist_albums, cx, window))
                    .into_any_element()
            }
            View::AlbumDetail => {
                let Some(album) = &self.state.current_album else {
                    return div().child("Album not found").into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(
                        h_flex()
                            .gap_5()
                            .items_end()
                            .child(self.render_cover(album.cover_art.as_deref(), 180.0, cx))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(album.name.clone()),
                                    )
                                    .child(div().text_color(cx.theme().muted_foreground).child(
                                        format!(
                                                "{}{}",
                                                album.artist,
                                                album
                                                    .year
                                                    .map(|year| format!(" - {year}"))
                                                    .unwrap_or_default()
                                            ),
                                    ))
                                    .child(
                                        Button::new("play-album")
                                            .label("Play album")
                                            .primary()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let songs = this.state.current_songs.clone();
                                                if !songs.is_empty() {
                                                    this.play_song_list(&songs, 0);
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .children(self.error_banner(cx))
                    .child(self.render_song_list(&self.state.current_songs, cx))
                    .into_any_element()
            }
            View::PlaylistDetail => {
                let Some(playlist) = &self.state.current_playlist else {
                    return div().child("Playlist not found").into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        playlist.name.clone(),
                        format!("{} tracks", self.state.playlist_songs.len()),
                        cx,
                    ))
                    .child(
                        h_flex().child(
                            Button::new("play-playlist")
                                .icon(PlayerIcon::Play)
                                .label("Play all")
                                .small()
                                .info()
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let songs = this.state.playlist_songs.clone();
                                    if !songs.is_empty() {
                                        this.play_song_list(&songs, 0);
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .children(self.error_banner(cx))
                    .child(self.render_song_list(&self.state.playlist_songs, cx))
                    .into_any_element()
            }
            View::NowPlaying => self.render_now_playing(window, cx),
            View::Queue => v_flex()
                .gap_5()
                .child(self.page_header(
                    "Queue",
                    format!("{} tracks in the current queue", self.state.queue.len()),
                    cx,
                ))
                .when(self.state.queue.is_empty(), |this| {
                    this.child(
                        div()
                            .py_8()
                            .text_color(cx.theme().muted_foreground)
                            .child("Start playing an album or playlist to build a queue."),
                    )
                })
                .when(!self.state.queue.is_empty(), |this| {
                    this.child(self.render_song_list(&self.state.queue, cx))
                })
                .into_any_element(),
        }
    }

    fn render_player(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let playback = self.audio.state();
        let duration = playback.duration.unwrap_or_default();
        let lyrics_open = self.state.view == View::NowPlaying;
        let queue_open = self.state.view == View::Queue;
        let song_info = if let Some(song) = &self.state.now_playing {
            h_flex()
                .id("now-playing-info")
                .w(px(260.0))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap_3()
                .p_1()
                .items_center()
                .cursor_pointer()
                .rounded(px(12.0))
                .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                .child(self.render_cover(song.cover_art.as_deref(), 60.0, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(song.title.clone()),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .overflow_hidden()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(song.artist.clone()),
                                )
                                .child(
                                    Button::new("player-lyrics-shortcut")
                                        .icon(AppIcon::Lyrics)
                                        .tooltip("Lyrics")
                                        .ghost()
                                        .with_size(px(26.0))
                                        .rounded_full()
                                        .selected(lyrics_open)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open_now_playing();
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_now_playing();
                    cx.notify();
                }))
                .into_any_element()
        } else {
            h_flex()
                .w(px(260.0))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap_3()
                .p_1()
                .items_center()
                .child(self.render_cover(None, 60.0, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child("No track playing"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child("Choose a song from your library."),
                        ),
                )
                .into_any_element()
        };

        let right_controls = h_flex()
            .w(px(260.0))
            .h(px(40.0))
            .min_w_0()
            .overflow_hidden()
            .flex_shrink_0()
            .justify_end()
            .items_center()
            .gap_2()
            .pl_4()
            .border_l_1()
            .border_color(cx.theme().border.opacity(0.8))
            .when_some(self.state.now_playing.as_ref(), |this, song| {
                this.child(
                    self.favorite_button(FavoriteKind::Song, &song.id, cx)
                        .with_size(px(30.0)),
                )
            })
            .child(self.playback_mode_button(cx))
            .child(
                Button::new("player-queue")
                    .icon(AppIcon::Queue)
                    .tooltip("Queue")
                    .ghost()
                    .with_size(px(30.0))
                    .rounded_full()
                    .selected(queue_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings_open = false;
                        this.state.view = View::Queue;
                        cx.notify();
                    })),
            );

        h_flex()
            .h(px(88.0))
            .px_4()
            .py_2()
            .gap_4()
            .items_center()
            .justify_between()
            .overflow_hidden()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.8))
            .bg(cx.theme().secondary.opacity(0.22))
            .child(song_info)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap_1()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                Button::new("previous")
                                    .icon(PlayerIcon::Previous)
                                    .tooltip("Previous track")
                                    .ghost()
                                    .with_size(px(28.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip(-1);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("play-pause")
                                    .icon(if playback.active && !playback.paused {
                                        PlayerIcon::Pause
                                    } else {
                                        PlayerIcon::Play
                                    })
                                    .tooltip(if playback.active && !playback.paused {
                                        "Pause"
                                    } else {
                                        "Play"
                                    })
                                    .info()
                                    .with_size(px(40.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_playback();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("next")
                                    .icon(PlayerIcon::Next)
                                    .tooltip("Next track")
                                    .ghost()
                                    .with_size(px(28.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip(1);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .h(px(24.0))
                            .max_w(px(760.0))
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .w(px(40.0))
                                    .flex_none()
                                    .text_right()
                                    .child(format_playback(playback.position)),
                            )
                            .child(
                                Slider::new(&self.playback_slider)
                                    .flex_1()
                                    .mx_1()
                                    .bg(cx.theme().info)
                                    .text_color(cx.theme().info)
                                    .rounded_full(),
                            )
                            .child(
                                div()
                                    .w(px(40.0))
                                    .flex_none()
                                    .child(format_playback(duration)),
                            ),
                    ),
            )
            .child(right_controls)
            .into_any_element()
    }
}

impl Render for NavidromeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let playback = self.audio.state();
        let progress = playback
            .duration
            .filter(|duration| !duration.is_zero())
            .map(|duration| {
                (playback.position.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
            })
            .unwrap_or_default();
        self.playback_slider.update(cx, |slider, cx| {
            let SliderValue::Single(current) = slider.value() else {
                return;
            };
            if (current - progress).abs() > 0.5 {
                slider.set_value(progress, window, cx);
            }
        });

        if self.state.settings_open {
            return v_flex()
                .size_full()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child(
                    TitleBar::new().child(
                        h_flex()
                            .h_full()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .pr_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Settings"),
                            )
                            .child(
                                Button::new("close-settings")
                                    .icon(AppIcon::Close)
                                    .tooltip("Close settings")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings_open = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_6()
                        .child(self.render_settings(cx)),
                )
                .into_any_element();
        }

        if self.state.view == View::NowPlaying {
            let (cover_base, cover_accent) = self
                .state
                .now_playing
                .as_ref()
                .and_then(|song| song.cover_art.as_deref())
                .and_then(|cover_id| self.state.cover_palettes.get(cover_id).copied())
                .unwrap_or((cx.theme().info, cx.theme().chart_2));
            let background_start = cx.theme().background.blend(cover_base.opacity(0.2));
            let background_end = cx.theme().background.blend(cover_accent.opacity(0.16));
            let background_animation = Animation::new(Duration::from_secs(18)).repeat();

            return v_flex()
                .size_full()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child(
                    TitleBar::new().child(
                        h_flex()
                            .h_full()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .pr_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Button::new("leave-now-playing")
                                            .icon(AppIcon::ArrowLeft)
                                            .tooltip("Back to library")
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.leave_now_playing();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Now Playing"),
                                    ),
                            )
                            .child(
                                Button::new("now-playing-settings")
                                    .icon(AppIcon::Settings)
                                    .tooltip("Settings")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings_open = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(self.render_now_playing(window, cx))
                        .with_animation(
                            "now-playing-background",
                            background_animation,
                            move |this, delta| {
                                this.bg(linear_gradient(
                                    120.0 + delta * 360.0,
                                    linear_color_stop(background_start, 0.0),
                                    linear_color_stop(background_end, 1.0),
                                ))
                            },
                        ),
                )
                .child(self.render_player(cx))
                .into_any_element();
        }

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .h_full()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .pr_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Navidrome Client"),
                        )
                        .child(
                            Button::new("title-settings")
                                .icon(AppIcon::Settings)
                                .tooltip("Settings")
                                .ghost()
                                .small()
                                .selected(self.state.settings_open)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.state.settings_open = !this.state.settings_open;
                                    cx.notify();
                                })),
                        ),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .w(px(216.0))
                            .h_full()
                            .p_3()
                            .gap_1()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().sidebar)
                            .child(
                                div()
                                    .px_2()
                                    .py_2()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().muted_foreground)
                                    .child("LIBRARY"),
                            )
                            .child(self.nav_button(View::Home, "Home", cx))
                            .child(self.nav_button(View::Favorites, "Favorites", cx))
                            .child(self.nav_button(View::Artists, "Artists", cx))
                            .child(self.nav_button(View::Albums, "Albums", cx))
                            .child(self.nav_button(View::Playlists, "Playlists", cx))
                            .child(self.nav_button(View::Search, "Search", cx)),
                    )
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .h_full()
                            .overflow_y_scroll()
                            .p_6()
                            .child(self.render_content(window, cx)),
                    ),
            )
            .child(self.render_player(cx))
            .into_any_element()
    }
}

fn vinyl_rotation_phase(position: Duration) -> f32 {
    (position.as_secs_f32() / 8.0).fract()
}

fn rgb_to_hsla(red: f32, green: f32, blue: f32) -> Hsla {
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) * 0.5;
    let delta = max - min;

    if delta <= f32::EPSILON {
        return hsla(0.0, 0.0, lightness, 1.0);
    }

    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(f32::EPSILON);
    let hue = if max == red {
        ((green - blue) / delta).rem_euclid(6.0)
    } else if max == green {
        (blue - red) / delta + 2.0
    } else {
        (red - green) / delta + 4.0
    } / 6.0;

    hsla(hue, saturation, lightness, 1.0)
}

fn extract_cover_palette(bytes: &[u8]) -> Option<(Hsla, Hsla)> {
    let image = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(32, 32)
        .to_rgb8();
    let mut average = [0.0_f32; 3];
    let mut vivid = [0.0_f32; 3];
    let mut count = 0.0_f32;
    let mut vivid_weight = 0.0_f32;

    for pixel in image.pixels() {
        let red = f32::from(pixel[0]) / 255.0;
        let green = f32::from(pixel[1]) / 255.0;
        let blue = f32::from(pixel[2]) / 255.0;
        average[0] += red;
        average[1] += green;
        average[2] += blue;
        count += 1.0;

        let color = rgb_to_hsla(red, green, blue);
        if color.s > 0.25 && color.l > 0.12 && color.l < 0.88 {
            let weight = color.s * (1.0 - (color.l - 0.5).abs());
            vivid[0] += red * weight;
            vivid[1] += green * weight;
            vivid[2] += blue * weight;
            vivid_weight += weight;
        }
    }

    if count == 0.0 {
        return None;
    }

    let mut base = rgb_to_hsla(average[0] / count, average[1] / count, average[2] / count);
    let mut accent = if vivid_weight > 0.0 {
        rgb_to_hsla(
            vivid[0] / vivid_weight,
            vivid[1] / vivid_weight,
            vivid[2] / vivid_weight,
        )
    } else {
        base
    };

    base.s = base.s.clamp(0.2, 0.72);
    base.l = base.l.clamp(0.35, 0.62);
    accent.s = (accent.s * 1.12).clamp(0.32, 0.86);
    accent.l = accent.l.clamp(0.38, 0.6);
    if (base.h - accent.h).abs() < 0.035 {
        accent.h = (accent.h + 0.08) % 1.0;
    }

    Some((base, accent))
}

fn gpui_image_format(format: image::ImageFormat) -> Option<GpuiImageFormat> {
    match format {
        image::ImageFormat::Png => Some(GpuiImageFormat::Png),
        image::ImageFormat::Jpeg => Some(GpuiImageFormat::Jpeg),
        image::ImageFormat::WebP => Some(GpuiImageFormat::Webp),
        image::ImageFormat::Gif => Some(GpuiImageFormat::Gif),
        image::ImageFormat::Bmp => Some(GpuiImageFormat::Bmp),
        image::ImageFormat::Tiff => Some(GpuiImageFormat::Tiff),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{extract_cover_palette, vinyl_rotation_phase};

    #[test]
    fn vinyl_rotation_tracks_playback_position() {
        assert!((vinyl_rotation_phase(Duration::from_secs(2)) - 0.25).abs() < f32::EPSILON);
        assert!((vinyl_rotation_phase(Duration::from_secs(10)) - 0.25).abs() < f32::EPSILON);
        assert!((vinyl_rotation_phase(Duration::from_secs(6)) - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn extracts_palette_from_default_cover() {
        let palette = extract_cover_palette(include_bytes!("../assets/default-cover.png"));
        assert!(palette.is_some());
    }
}
