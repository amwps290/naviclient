//! Cross-source local playlists UI: detail page, dialogs, card grid, and `.m3u8` import/export.

use std::path::PathBuf;

use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::menu::PopupMenuItem;

use crate::app::prelude::*;
use crate::app::widgets::detail_hero::DetailHero;
use crate::app::widgets::shelf::cover_grid;
use crate::local_playlists::LocalPlaylist;

#[derive(Default)]
pub(in crate::app) struct LocalPlaylistUi {
    pub(in crate::app) dialog: Option<LocalPlaylistDialog>,
    pub(in crate::app) error: Option<String>,
    pub(in crate::app) input_subscription: Option<Subscription>,
}

impl LocalPlaylistUi {
    pub(in crate::app) fn close_dialog(&mut self) -> bool {
        let open = self.dialog.take().is_some();
        self.input_subscription = None;
        self.error = None;
        open
    }
}

pub(in crate::app) enum LocalPlaylistDialog {
    Name {
        playlist_id: Option<String>,
        songs: Vec<Song>,
        input: Entity<InputState>,
    },
    Choose {
        songs: Vec<Song>,
    },
    ConfirmDelete {
        playlist_id: String,
        playlist_name: String,
    },
}

impl NavidromeApp {
    pub(in crate::app) fn open_local_playlist(&mut self, id: &str) {
        self.record_nav_location(crate::app::shell::history::NavLocation::local_playlist(id));
        self.open_local_playlist_without_history(id);
    }

    pub(in crate::app) fn open_local_playlist_without_history(&mut self, id: &str) {
        self.clear_song_multi();
        self.state.clear_song_selection_for_scope(SongScope::LocalPlaylist);
        if let Some(playlist) = self.local_playlists.get(id).cloned() {
            for song in playlist.songs.iter().take(40) {
                self.ensure_song_cover(song, false);
            }
            if let Some(cover_song) = playlist.cover_song() {
                self.ensure_song_cover(cover_song, true);
            }
            self.state.active_local_playlist = Some(playlist);
            self.state.navigate_to(View::LocalPlaylistDetail);
        } else {
            self.state.active_local_playlist = None;
            self.state.navigate_to(View::Playlists);
        }
    }

    fn sync_active_local_playlist(&mut self, id: &str) {
        if self
            .state
            .active_local_playlist
            .as_ref()
            .is_some_and(|active| active.id == id)
        {
            self.state.active_local_playlist = self.local_playlists.get(id).cloned();
            if self.state.active_local_playlist.is_none()
                && self.state.view == View::LocalPlaylistDetail
            {
                self.state.view = View::Playlists;
            }
        }
    }

    pub(in crate::app) fn play_local_playlist(&mut self, id: &str, index: usize) {
        let Some(songs) = self
            .local_playlists
            .get(id)
            .map(|playlist| playlist.songs.clone())
        else {
            return;
        };
        if songs.is_empty() {
            return;
        }
        self.replace_queue_and_play_mixed(songs, index, QueueOrigin::Playlist);
    }

    pub(in crate::app) fn move_local_playlist_song(&mut self, id: &str, from: usize, to: usize) {
        match self.local_playlists.move_song(id, from, to) {
            Ok(_) => {
                self.sync_active_local_playlist(id);
            }
            Err(error) => {
                self.notify_error(t!(
                    "Failed to update local playlist: %{error}",
                    error = format!("{error:#}")
                ));
            }
        }
    }

    pub(in crate::app) fn remove_local_playlist_song(&mut self, id: &str, index: usize) {
        match self.local_playlists.remove_song(id, index) {
            Ok(_) => {
                self.state
                    .clear_song_selection_for_scope(SongScope::LocalPlaylist);
                self.sync_active_local_playlist(id);
            }
            Err(error) => {
                self.notify_error(t!(
                    "Failed to update local playlist: %{error}",
                    error = format!("{error:#}")
                ));
            }
        }
    }

    pub(in crate::app) fn name_local_playlist(
        &mut self,
        playlist: Option<&LocalPlaylist>,
        songs: Vec<Song>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = playlist.map_or("", |p| p.name.as_str()).to_string();
        let playlist_id = playlist.map(|p| p.id.clone());
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Playlist name"))
                .default_value(initial)
        });
        self.local_playlist_ui.input_subscription = Some(cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_local_playlist_dialog(window, cx);
                }
            },
        ));
        self.local_playlist_ui.dialog = Some(LocalPlaylistDialog::Name {
            playlist_id,
            songs,
            input: input.clone(),
        });
        self.local_playlist_ui.error = None;
        self.show_local_playlist_dialog(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(in crate::app) fn choose_local_playlist(
        &mut self,
        songs: Vec<Song>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if songs.is_empty() {
            return;
        }
        let prepared: Vec<Song> = songs
            .into_iter()
            .map(|mut song| {
                if song.source.is_none() {
                    song.source = self.api.as_ref().map(|api| api.id().clone());
                }
                song
            })
            .collect();
        self.local_playlist_ui.dialog = Some(LocalPlaylistDialog::Choose { songs: prepared });
        self.local_playlist_ui.error = None;
        self.show_local_playlist_dialog(window, cx);
    }

    pub(in crate::app) fn confirm_delete_local_playlist(
        &mut self,
        playlist: &LocalPlaylist,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.local_playlist_ui.dialog = Some(LocalPlaylistDialog::ConfirmDelete {
            playlist_id: playlist.id.clone(),
            playlist_name: playlist.name.clone(),
        });
        self.local_playlist_ui.error = None;
        self.show_local_playlist_dialog(window, cx);
    }

    fn show_local_playlist_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let render_app = app.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| {
                            let dialog = this.render_local_playlist_dialog(dialog, cx);
                            crate::app::style::surface::raised_dialog(dialog, cx)
                        })
                        .expect("app owns local playlist dialog")
                });
                let _ = app.update(cx, |this, cx| {
                    if let Some(LocalPlaylistDialog::Name { input, .. }) =
                        &this.local_playlist_ui.dialog
                    {
                        input.update(cx, |input, cx| input.focus(window, cx));
                    }
                });
            });
        }
        cx.notify();
    }

    fn submit_local_playlist_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.apply_local_playlist_dialog(cx) {
            self.local_playlist_ui.close_dialog();
            window.close_dialog(cx);
        }
        cx.notify();
    }

    fn apply_local_playlist_dialog(&mut self, cx: &mut Context<Self>) -> bool {
        match &self.local_playlist_ui.dialog {
            Some(LocalPlaylistDialog::Name {
                playlist_id,
                songs,
                input,
            }) => {
                let name = input.read(cx).value().trim().to_string();
                if name.is_empty() {
                    self.local_playlist_ui.error = Some(t!("Enter a playlist name.").to_string());
                    return false;
                }
                if let Some(id) = playlist_id.clone() {
                    match self.local_playlists.rename(&id, &name) {
                        Ok(_) => {
                            self.sync_active_local_playlist(&id);
                            self.notify_success(t!(
                                "Renamed local playlist to “%{name}”",
                                name = name
                            ));
                            true
                        }
                        Err(error) => {
                            self.local_playlist_ui.error = Some(format!("{error:#}"));
                            false
                        }
                    }
                } else {
                    let tracks = songs.clone();
                    match self.local_playlists.create(&name, tracks) {
                        Ok(created) => {
                            self.notify_success(t!(
                                "Created local playlist “%{name}”",
                                name = created.name
                            ));
                            true
                        }
                        Err(error) => {
                            self.local_playlist_ui.error = Some(format!("{error:#}"));
                            false
                        }
                    }
                }
            }
            Some(LocalPlaylistDialog::ConfirmDelete {
                playlist_id,
                playlist_name,
            }) => {
                let id = playlist_id.clone();
                let name = playlist_name.clone();
                match self.local_playlists.delete(&id) {
                    Ok(()) => {
                        self.sync_active_local_playlist(&id);
                        self.notify_success(t!("Deleted local playlist “%{name}”", name = name));
                        true
                    }
                    Err(error) => {
                        self.local_playlist_ui.error = Some(format!("{error:#}"));
                        false
                    }
                }
            }
            _ => false,
        }
    }

    fn render_local_playlist_dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let panel_bg = if dark {
            hsla(0.0, 0.0, 1.0, 0.035)
        } else {
            p.sunken.opacity(0.65)
        };
        let panel_border = if dark {
            hsla(0.0, 0.0, 1.0, 0.08)
        } else {
            p.border.opacity(0.82)
        };
        let mut content = v_flex().gap_3();
        let (title, action, destructive) = match &self.local_playlist_ui.dialog {
            Some(LocalPlaylistDialog::Name {
                playlist_id,
                songs,
                input,
            }) => {
                let mut card = v_flex()
                    .w_full()
                    .p_3()
                    .gap_2p5()
                    .rounded(px(tokens::radius::COVER))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border);
                if playlist_id.is_none() && !songs.is_empty() {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(p.text_secondary)
                            .child(t!(
                                "Save %{count} tracks in their current order.",
                                count = songs.len()
                            )),
                    );
                }
                card = card.child(Input::new(input).w_full());
                content = content.child(card);
                (
                    if playlist_id.is_some() {
                        t!("Rename local playlist")
                    } else {
                        t!("New local playlist")
                    }
                    .to_string(),
                    Some(t!("Save").to_string()),
                    false,
                )
            }
            Some(LocalPlaylistDialog::Choose { songs }) => {
                let new_songs = songs.clone();
                content = content.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_2p5()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .rounded(px(tokens::radius::COVER))
                        .bg(panel_bg)
                        .border_1()
                        .border_color(panel_border)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(p.text_secondary)
                                .child(t!(
                                    "Add %{count} tracks to a cross-source local playlist.",
                                    count = songs.len()
                                )),
                        )
                        .child(
                            surface_button("new-local-playlist-from-songs")
                                .label(t!("New local playlist…"))
                                .compact_action()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.name_local_playlist(None, new_songs.clone(), window, cx);
                                })),
                        ),
                );
                let playlists = self.local_playlists.list();
                let mut picker = v_flex()
                    .id("local-playlist-picker")
                    .max_h(px(280.0))
                    .overflow_y_scroll()
                    .p_1p5()
                    .gap_1()
                    .rounded(px(tokens::radius::COVER))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border);
                if playlists.is_empty() {
                    picker = picker.child(
                        div()
                            .py_6()
                            .px_4()
                            .text_center()
                            .text_sm()
                            .text_color(p.text_muted)
                            .child(t!("No local playlists yet. Create one above.")),
                    );
                } else {
                    picker = picker.children(playlists.iter().map(|playlist| {
                        let id = playlist.id.clone();
                        let name = playlist.name.clone();
                        let count = playlist.song_count();
                        let songs = songs.clone();
                        let cover = self.render_local_playlist_cover(playlist, 34.0, tokens::radius::CONTROL, Vec::new());
                        ghost_button(SharedString::from(format!("choose-local-playlist-{id}")))
                            .w_full()
                            .h(px(48.0))
                            .px_2p5()
                            .rounded(px(tokens::radius::ROW))
                            .justify_start()
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .items_center()
                                    .gap_2p5()
                                    .child(cover)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.text)
                                            .truncate()
                                            .child(name.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_xs()
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .text_color(p.text_muted)
                                            .child(t!("%{count} tracks", count = count)),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let added = songs.len();
                                match this.local_playlists.append_songs(&id, songs.clone()) {
                                    Ok(_) => {
                                        this.sync_active_local_playlist(&id);
                                        this.notify_success(t!(
                                            "Added %{count} tracks to “%{name}”",
                                            count = added,
                                            name = name
                                        ));
                                        this.local_playlist_ui.close_dialog();
                                        window.close_dialog(cx);
                                    }
                                    Err(error) => {
                                        this.local_playlist_ui.error = Some(format!("{error:#}"));
                                    }
                                }
                                cx.notify();
                            }))
                    }));
                }
                content = content.child(picker);
                (t!("Add to local playlist").to_string(), None, false)
            }
            Some(LocalPlaylistDialog::ConfirmDelete { playlist_name, .. }) => {
                content = content.child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded(px(tokens::radius::ROW))
                        .bg(panel_bg)
                        .border_1()
                        .border_color(panel_border)
                        .text_sm()
                        .text_color(p.text_secondary)
                        .child(t!(
                            "Delete local playlist “%{name}”? Your music files and remote playlists will not be affected.",
                            name = playlist_name
                        )),
                );
                (
                    t!("Delete local playlist?").to_string(),
                    Some(t!("Delete").to_string()),
                    true,
                )
            }
            None => return dialog,
        };
        if let Some(error) = &self.local_playlist_ui.error {
            content = content.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(px(tokens::radius::ROW))
                    .bg(cx.theme().danger.opacity(0.09))
                    .border_1()
                    .border_color(cx.theme().danger.opacity(0.22))
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                ghost_button("cancel-local-playlist-dialog")
                    .label(t!("Cancel"))
                    .dialog_action()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.local_playlist_ui.close_dialog();
                        window.close_dialog(cx);
                        cx.notify();
                    })),
            )
            .when_some(action, |footer, action| {
                let button = if destructive {
                    danger_button("save-local-playlist-dialog")
                } else {
                    primary_button("save-local-playlist-dialog")
                };
                footer.child(
                    button
                        .label(action)
                        .dialog_action()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.submit_local_playlist_dialog(window, cx);
                        })),
                )
            });
        let submit = cx.entity().downgrade();
        dialog
            .title(title)
            .w(px(440.0))
            .overlay_closable(false)
            .close_button(true)
            .keyboard(true)
            .on_ok(move |_, window, cx| {
                let _ = submit.update(cx, |this, cx| {
                    this.submit_local_playlist_dialog(window, cx);
                });
                false
            })
            .on_close(cx.listener(|this, _, _, cx| {
                this.local_playlist_ui.close_dialog();
                cx.notify();
            }))
            .child(content)
            .footer(footer)
    }

    /// Open a native file picker to import one or more `.m3u8` / `.m3u` files as local playlists.
    pub(in crate::app) fn prompt_import_m3u8(&mut self, cx: &mut Context<Self>) {
        let local_sources: Vec<(crate::source::SourceId, Vec<PathBuf>)> = self
            .config
            .sources
            .iter()
            .filter_map(|profile| match profile {
                crate::source::SourceProfile::LocalFolder { id, roots, .. } => Some((
                    crate::source::SourceId::new(crate::source::ProviderKind::LocalFolder, id),
                    roots.clone(),
                )),
                _ => None,
            })
            .collect();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(t!("Select .m3u8 playlist files").into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver.await.ok().and_then(Result::ok).flatten();
            if let Some(paths) = selected {
                let mut imported = Vec::new();
                let mut errors = Vec::new();
                for path in paths {
                    match crate::local_playlists::import_m3u8(&path, &local_sources) {
                        Ok(result) => imported.push(result),
                        Err(error) => errors.push(format!("{}: {error:#}", path.display())),
                    }
                }
                let _ = this.update(cx, |this, cx| {
                    let mut created_count = 0usize;
                    let mut skipped_total = 0usize;
                    for item in imported {
                        skipped_total += item.skipped_missing;
                        if item.songs.is_empty() {
                            errors.push(
                                t!(
                                    "“%{name}” had no playable tracks.",
                                    name = item.name
                                )
                                .to_string(),
                            );
                            continue;
                        }
                        match this.local_playlists.create(&item.name, item.songs) {
                            Ok(_) => created_count += 1,
                            Err(error) => errors.push(format!("{error:#}")),
                        }
                    }
                    if created_count > 0 {
                        if skipped_total > 0 {
                            this.notify_success(t!(
                                "Imported %{count} local playlist(s) (%{skipped} missing files skipped)",
                                count = created_count,
                                skipped = skipped_total
                            ));
                        } else {
                            this.notify_success(t!(
                                "Imported %{count} local playlist(s)",
                                count = created_count
                            ));
                        }
                    }
                    if let Some(first_err) = errors.first() {
                        this.notify_error(t!(
                            "M3U8 import warning: %{error}",
                            error = first_err
                        ));
                    }
                    cx.notify();
                });
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    /// Open a native save dialog to export a local playlist as UTF-8 `.m3u8`.
    pub(in crate::app) fn prompt_export_m3u8(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(playlist) = self.local_playlists.get(id).cloned() else {
            return;
        };
        let suggested = format!("{}.m3u8", sanitize_filename(&playlist.name));
        let default_dir = directories::UserDirs::new()
            .map(|dirs| {
                dirs.audio_dir()
                    .unwrap_or_else(|| dirs.home_dir())
                    .to_path_buf()
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let offline_paths: Vec<Option<PathBuf>> = playlist
            .songs
            .iter()
            .map(|song| {
                let media = song.media_key()?;
                self.offline_store.audio_path(&media)
            })
            .collect();
        let receiver = cx.prompt_for_new_path(&default_dir, Some(&suggested));
        cx.spawn(async move |this, cx| {
            let selected = receiver.await.ok().and_then(Result::ok).flatten();
            if let Some(mut path) = selected {
                if path.extension().is_none() {
                    path.set_extension("m3u8");
                }
                let result = crate::local_playlists::m3u8::export_m3u8_to_file(
                    &playlist,
                    &path,
                    |song| {
                        playlist
                            .songs
                            .iter()
                            .position(|candidate| {
                                candidate.id == song.id && candidate.source == song.source
                            })
                            .and_then(|idx| offline_paths.get(idx).cloned().flatten())
                            .or_else(|| {
                                song.path
                                    .as_deref()
                                    .map(PathBuf::from)
                                    .filter(|p| p.is_file())
                            })
                    },
                );
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(()) => {
                            this.notify_success(t!(
                                "Exported “%{name}” to %{path}",
                                name = playlist.name,
                                path = path.display()
                            ));
                        }
                        Err(error) => {
                            this.notify_error(t!(
                                "Export failed: %{error}",
                                error = format!("{error:#}")
                            ));
                        }
                    }
                    cx.notify();
                });
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(in crate::app) fn render_local_playlist_cover(
        &self,
        playlist: &LocalPlaylist,
        size_px: f32,
        radius_px: f32,
        shadow: Vec<gpui_kit::BoxShadow>,
    ) -> gpui_kit::AnyElement {
        let cover = playlist
            .cover_song()
            .and_then(|song| self.song_cover_image(song))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.render_cover_image(cover, size_px, radius_px, shadow)
    }

    pub(in crate::app) fn render_local_playlists_section(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let columns = self.content_columns(window);
        let playlists = self.local_playlists.list();

        let grid = cover_grid(
            columns,
            playlists
                .iter()
                .map(|playlist| {
                    let id_click = playlist.id.clone();
                    let id_play = playlist.id.clone();
                    let playlist_menu = playlist.clone();
                    let menu_app = cx.entity().downgrade();
                    let hover_key = format!("local-playlist-card-{}", playlist.id);
                    let hovered = self.state.hovered_item.as_deref() == Some(hover_key.as_str());
                    let hover_key_cb = hover_key.clone();
                    let subtitle = local_playlist_card_subtitle(playlist);

                    v_flex()
                        .id(SharedString::from(hover_key))
                        .min_w_0()
                        .gap_1()
                        .cursor_pointer()
                        .on_hover(cx.listener(move |this, hovering, _, cx| {
                            this.set_hovered(&hover_key_cb, *hovering, cx);
                        }))
                        .child(
                            div()
                                .relative()
                                .w(px(columns.width))
                                .h(px(columns.width))
                                .mb_1()
                                .child(self.render_local_playlist_cover(
                                    playlist,
                                    columns.width,
                                    tokens::radius::COVER,
                                    cover_shadow(hovered, cx),
                                ))
                                .when(hovered, |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .right_2()
                                            .bottom_2()
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .child(
                                                round_play_button(
                                                    SharedString::from(format!(
                                                        "local-playlist-play-{}",
                                                        playlist.id
                                                    )),
                                                    PlayerIcon::Play,
                                                    t!("Play all"),
                                                    tokens::HOVER_PLAY_BUTTON,
                                                    cx,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.play_local_playlist(&id_play, 0);
                                                    cx.notify();
                                                })),
                                            ),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(playlist.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(subtitle),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_local_playlist(&id_click);
                            cx.notify();
                        }))
                        .context_menu(move |menu, _, cx| {
                            let playlist = playlist_menu.clone();
                            menu_app
                                .update(cx, |_this, cx| {
                                    let play_id = playlist.id.clone();
                                    let export_id = playlist.id.clone();
                                    let rename = playlist.clone();
                                    let delete = playlist.clone();
                                    let empty = playlist.songs.is_empty();
                                    menu.item(
                                        PopupMenuItem::new(t!("Play all"))
                                            .disabled(empty)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.play_local_playlist(&play_id, 0);
                                                cx.notify();
                                            })),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("Export .m3u8…")).on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.prompt_export_m3u8(&export_id, cx);
                                            }),
                                        ),
                                    )
                                    .separator()
                                    .item(
                                        PopupMenuItem::new(t!("Rename")).on_click(cx.listener(
                                            move |this, _, window, cx| {
                                                this.name_local_playlist(
                                                    Some(&rename),
                                                    vec![],
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("Delete")).on_click(cx.listener(
                                            move |this, _, window, cx| {
                                                this.confirm_delete_local_playlist(
                                                    &delete, window, cx,
                                                );
                                            },
                                        )),
                                    )
                                })
                                .expect("app owns local playlist card menu")
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
        );

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(tokens::SECTION_TITLE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(t!("Local playlists")),
                            )
                            .child(
                                div()
                                    .text_size(px(tokens::CAPTION))
                                    .text_color(p.text_muted)
                                    .child(t!("Cross-source playlists stored locally · supports .m3u8")),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                primary_button("new-local-playlist")
                                    .label(t!("New local playlist"))
                                    .compact_action()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.name_local_playlist(None, vec![], window, cx);
                                    })),
                            )
                            .child(
                                surface_button("import-m3u8-playlist")
                                    .label(t!("Import .m3u8"))
                                    .compact_action()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.prompt_import_m3u8(cx);
                                    })),
                            ),
                    ),
            )
            .when(playlists.is_empty(), |this| {
                this.child(
                    div()
                        .w_full()
                        .py_6()
                        .px_4()
                        .rounded(px(tokens::radius::COVER))
                        .bg(p.sunken.opacity(0.6))
                        .border_1()
                        .border_color(p.border.opacity(0.6))
                        .text_sm()
                        .text_color(p.text_muted)
                        .child(t!(
                            "No local playlists yet. Create one, import a .m3u8 file, or add any song via its menu."
                        )),
                )
            })
            .when(!playlists.is_empty(), |this| this.child(grid))
            .into_any_element()
    }

    pub(in crate::app) fn render_local_playlist_detail(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(playlist) = self.state.active_local_playlist.as_ref() else {
            return div().child(t!("Playlist not found")).into_any_element();
        };
        let play_id = playlist.id.clone();
        let export_id = playlist.id.clone();
        let rename_playlist = playlist.clone();
        let delete_playlist = playlist.clone();
        let empty = playlist.songs.is_empty();
        let layout = self.shell_layout(window);
        let padding = layout.page_padding;
        let player_clearance = layout.player_clearance();

        let cover_song = playlist.cover_song().cloned();
        let cover_id = cover_song.as_ref().and_then(|song| song.cover_art.clone());
        let subtitle = local_playlist_card_subtitle(playlist);

        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Local playlist").into(),
                title: playlist.name.clone().into(),
                cover_id: if cover_song
                    .as_ref()
                    .is_some_and(|song| self.browsing_owns_song(song))
                {
                    cover_id
                } else {
                    None
                },
                round_cover: false,
                wide_cover: false,
                meta: div().child(subtitle).into_any_element(),
                actions: h_flex()
                    .w_full()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-local-playlist",
                            PlayerIcon::Play,
                            t!("Play all"),
                            tokens::DETAIL_PLAY_BUTTON,
                            cx,
                        )
                        .disabled(empty)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.play_local_playlist(&play_id, 0);
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("export-local-playlist-m3u8")
                            .icon(AppIcon::Download)
                            .label(t!("Export .m3u8"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.prompt_export_m3u8(&export_id, cx);
                            })),
                    )
                    .child(
                        ghost_button("rename-local-playlist")
                            .label(t!("Rename"))
                            .rounded(px(tokens::radius::FULL))
                            .compact_action()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.name_local_playlist(
                                    Some(&rename_playlist),
                                    vec![],
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        ghost_button("delete-local-playlist")
                            .label(t!("Delete"))
                            .rounded(px(tokens::radius::FULL))
                            .compact_action()
                            .text_color(cx.theme().danger)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_delete_local_playlist(&delete_playlist, window, cx);
                            })),
                    )
                    .into_any_element(),
            },
            window,
            cx,
        );

        v_flex()
            .size_full()
            .min_h_0()
            .child(hero)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(padding))
                    .pb(px(player_clearance))
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .text_color(palette(cx).text_muted)
                            .child(if empty {
                                t!("This local playlist is empty. Use any song’s menu to add tracks from any source.")
                            } else {
                                t!("Use a song’s menu to reorder or remove tracks.")
                            }),
                    )
                    .child(if empty {
                        crate::app::songs::view::render_empty_song_list(
                            t!("No tracks here yet").to_string(),
                            t!("Add tracks from any music source using “Add to local playlist…”.")
                                .to_string(),
                            cx,
                        )
                    } else {
                        self.render_song_list(SongScope::LocalPlaylist, cx)
                    }),
            )
            .into_any_element()
    }
}

fn local_playlist_card_subtitle(playlist: &LocalPlaylist) -> String {
    let mut parts = vec![t!("%{track_count} tracks", track_count = playlist.song_count()).to_string()];
    if let Some(duration) = playlist.total_duration_secs() {
        parts.push(format_duration(Some(duration)));
    }
    let sources = playlist.distinct_sources().len();
    if sources > 1 {
        parts.push(t!("%{count} sources", count = sources).to_string());
    }
    parts.join(" · ")
}

fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "playlist".to_string()
    } else {
        trimmed.to_string()
    }
}
