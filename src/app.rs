use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, img, px, rems, Animation, AnimationExt, AppContext, Context, Entity, FontWeight,
    Image as GpuiImage, ImageFormat as GpuiImageFormat, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    v_flex, ActiveTheme, Selectable, Theme, ThemeMode, TitleBar,
};
use smol::Timer;
use tokio::runtime::Runtime;

use crate::api::{format_duration, Api};
use crate::audio::{format_playback, AudioHandle};
use crate::config;
use crate::models::{
    Album, Artist, Config, Playlist, SearchResults, ServerInfo, Song, ThemePreference,
};
use crate::msg::{error_message, Msg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Home,
    Artists,
    Albums,
    Playlists,
    Search,
    ArtistDetail,
    AlbumDetail,
    PlaylistDetail,
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
    current_playlist: Option<Playlist>,
    playlist_songs: Vec<Song>,
    search_results: Option<SearchResults>,
    queue: Vec<Song>,
    queue_index: Option<usize>,
    now_playing: Option<Song>,
    covers: HashMap<String, Arc<GpuiImage>>,
    requested_covers: HashSet<String>,
    status: String,
    error: Option<String>,
    settings_open: bool,
    volume: f32,
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
            current_playlist: None,
            playlist_songs: Vec::new(),
            search_results: None,
            queue: Vec::new(),
            queue_index: None,
            now_playing: None,
            covers: HashMap::new(),
            requested_covers: HashSet::new(),
            status: "Not connected".to_string(),
            error: None,
            settings_open: false,
            volume: 0.8,
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
    state: AppState,
    search_input: Entity<InputState>,
    server_input: Entity<InputState>,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    playback_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    _subscriptions: Vec<Subscription>,
}

impl NavidromeApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = config::load();
        let api = Api::new(&config.server_url, &config.username, &config.password).ok();
        let (tx, rx) = mpsc::channel();
        let runtime = Runtime::new().expect("failed to create Tokio runtime");
        let audio = AudioHandle::start().expect("failed to start audio worker");
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
        let volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(80.0)
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
            state: AppState::default(),
            search_input: search_input.clone(),
            server_input,
            username_input,
            password_input,
            playback_slider: playback_slider.clone(),
            volume_slider: volume_slider.clone(),
            _subscriptions: Vec::new(),
        };
        app.audio.set_volume(app.state.volume);
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
        app._subscriptions.push(cx.subscribe(
            &volume_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.state.volume = (*value / 100.0).clamp(0.0, 1.0);
                this.audio.set_volume(this.state.volume);
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
            Timer::after(Duration::from_millis(120)).await;
            if this
                .update(cx, |this, cx| {
                    this.poll_messages();
                    this.handle_playback_end();
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

    fn play_queue_index(&mut self, index: usize) {
        let Some(song) = self.state.queue.get(index).cloned() else {
            return;
        };
        self.state.queue_index = Some(index);
        self.state.now_playing = Some(song.clone());
        self.state.ended_handled = false;
        self.ensure_cover(song.cover_art.as_deref());
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

    fn skip(&mut self, offset: i32) {
        let Some(index) = self.state.queue_index else {
            return;
        };
        let len = self.state.queue.len();
        if len == 0 {
            return;
        }
        let next = (index as i32 + offset).rem_euclid(len as i32) as usize;
        self.play_queue_index(next);
    }

    fn stop_playback(&mut self) {
        self.audio.stop();
        self.state.now_playing = None;
        self.state.ended_handled = true;
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
        self.state = AppState {
            volume: self.state.volume,
            ..AppState::default()
        };
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
                                self.state.current_songs = songs;
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
                Msg::PlaylistSongs {
                    playlist_id,
                    result,
                } => {
                    self.state.loading = false;
                    if self.state.current_playlist.as_ref().map(|p| p.id.as_str())
                        == Some(playlist_id.as_str())
                    {
                        match result {
                            Ok(songs) => self.state.playlist_songs = songs,
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
                Msg::Cover { id, result } => {
                    if let Ok(bytes) = result {
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
        if playback.ended && !self.state.ended_handled && self.state.now_playing.is_some() {
            self.state.ended_handled = true;
            self.skip(1);
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
                    _ => {}
                }
                cx.notify();
            }));
        button.ghost().selected(selected)
    }

    fn render_cover(
        &self,
        cover_id: Option<&str>,
        size: f32,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        if let Some(image) = cover_id.and_then(|id| self.state.covers.get(id)) {
            return img(image.clone())
                .w(px(size))
                .h(px(size))
                .rounded_lg()
                .into_any_element();
        }
        div()
            .w(px(size))
            .h(px(size))
            .rounded_lg()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().muted_foreground)
            .flex()
            .items_center()
            .justify_center()
            .child("No cover")
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
                    .child(self.render_cover(album.cover_art.as_deref(), 176.0, cx))
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
                    .child(self.render_cover(artist.cover_art.as_deref(), 152.0, cx))
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

    fn render_song_list(&self, songs: &[Song], cx: &Context<Self>) -> gpui::AnyElement {
        let queue_source = songs.to_vec();
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
                    .child(div().w(px(54.0)))
                    .child(div().w(px(48.0)).child("#"))
                    .child(div().flex_1().child("Title"))
                    .child(div().w(px(220.0)).child("Artist"))
                    .child(div().w(px(72.0)).child("Time")),
            )
            .children(songs.iter().enumerate().map(|(index, song)| {
                let queue = queue_source.clone();
                let current = self
                    .state
                    .now_playing
                    .as_ref()
                    .is_some_and(|playing| playing.id == song.id);
                h_flex()
                    .min_h(px(44.0))
                    .px_3()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                    .child(
                        Button::new(SharedString::from(format!("play-{}", song.id)))
                            .label(if current { "Playing" } else { "Play" })
                            .compact()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.play_song_list(&queue, index);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .w(px(48.0))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                song.track
                                    .map(|track| track.to_string())
                                    .unwrap_or_else(|| "-".to_string()),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(song.title.clone()),
                    )
                    .child(
                        div()
                            .w(px(220.0))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(song.artist.clone()),
                    )
                    .child(
                        div()
                            .w(px(72.0))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format_duration(song.duration)),
                    )
            }))
            .into_any_element()
    }

    fn render_settings(&self, cx: &Context<Self>) -> gpui::AnyElement {
        v_flex()
            .max_w(px(680.0))
            .gap_5()
            .child(self.page_header(
                "Server settings",
                "Connect this client to a Navidrome or Subsonic-compatible server.",
                cx,
            ))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Appearance"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Choose a light or dark theme, or follow the system setting."),
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
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.set_theme(preference, window, cx);
                                    },
                                ))
                            }),
                        ),
                    ),
            )
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
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-settings")
                            .label("Save and connect")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_settings(cx);
                                this.state.settings_open = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("cancel-settings")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.settings_open = false;
                                cx.notify();
                            })),
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
            View::Playlists => v_flex()
                .gap_5()
                .child(self.page_header(
                    "Playlists",
                    format!("{} playlists", self.state.playlists.len()),
                    cx,
                ))
                .children(self.error_banner(cx))
                .children(self.state.playlists.iter().map(|playlist| {
                    let playlist_for_click = playlist.clone();
                    h_flex()
                        .min_h(px(54.0))
                        .px_3()
                        .justify_between()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(playlist.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{} tracks",
                                            playlist.song_count.unwrap_or_default()
                                        )),
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("playlist-{}", playlist.id)))
                                .label("Open")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_playlist(playlist_for_click.clone());
                                    cx.notify();
                                })),
                        )
                }))
                .into_any_element(),
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
                        Button::new("play-playlist")
                            .label("Play playlist")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let songs = this.state.playlist_songs.clone();
                                if !songs.is_empty() {
                                    this.play_song_list(&songs, 0);
                                }
                                cx.notify();
                            })),
                    )
                    .children(self.error_banner(cx))
                    .child(self.render_song_list(&self.state.playlist_songs, cx))
                    .into_any_element()
            }
        }
    }

    fn render_player(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let playback = self.audio.state();
        let song_info = if let Some(song) = &self.state.now_playing {
            h_flex()
                .w(px(320.0))
                .gap_3()
                .child(self.render_cover(song.cover_art.as_deref(), 52.0, cx))
                .child(
                    v_flex()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(song.title.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(song.artist.clone()),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .w(px(320.0))
                .text_color(cx.theme().muted_foreground)
                .child("No track playing")
                .into_any_element()
        };
        let duration = playback.duration.unwrap_or_default();
        h_flex()
            .h(px(84.0))
            .px_4()
            .gap_5()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(song_info)
            .child(
                v_flex()
                    .flex_1()
                    .gap_2()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Button::new("previous").label("Prev").compact().on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.skip(-1);
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("play-pause")
                                    .label(if playback.active && !playback.paused {
                                        "Pause"
                                    } else {
                                        "Play"
                                    })
                                    .primary()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_playback();
                                        cx.notify();
                                    })),
                            )
                            .child(Button::new("next").label("Next").compact().on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.skip(1);
                                    cx.notify();
                                }),
                            ))
                            .child(Button::new("stop").label("Stop").compact().on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.stop_playback();
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format_playback(playback.position))
                            .child(Slider::new(&self.playback_slider).flex_1())
                            .child(format_playback(duration)),
                    ),
            )
            .child(
                h_flex()
                    .w(px(190.0))
                    .gap_2()
                    .child(div().text_xs().child("Volume"))
                    .child(Slider::new(&self.volume_slider).flex_1()),
            )
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

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex().h_full().items_center().child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Navidrome Client"),
                    ),
                ),
            )
            .child(
                h_flex()
                    .h(px(54.0))
                    .px_4()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Navidrome"),
                            )
                            .child(
                                div()
                                    .max_w(px(620.0))
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(self.state.status.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("refresh")
                                    .label("Refresh")
                                    .loading(self.state.loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_library();
                                        cx.notify();
                                    })),
                            )
                            .child(Button::new("settings").label("Settings").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.state.settings_open = true;
                                    cx.notify();
                                }),
                            )),
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
    }
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
