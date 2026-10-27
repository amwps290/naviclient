use super::*;
use crate::playlist::{can_edit, song_ids, PlaylistEdit, PlaylistEdited};
use gpui_kit::component::{
    dialog::Dialog,
    menu::{PopupMenu, PopupMenuItem},
    notification::Notification,
};
use gpui_kit::{FocusHandle, Focusable};

#[derive(Default)]
pub(super) struct PlaylistUi {
    pub generation: u64,
    pub list_request: u64,
    pub detail_request: u64,
    pub list_loading: bool,
    pub detail_loading: bool,
    pub busy: bool,
    pub menu_focus: Option<FocusHandle>,
    dialog: Option<PlaylistDialog>,
    error: Option<String>,
    input_subscription: Option<Subscription>,
}

impl PlaylistUi {
    /// 对话框是否打开（导入向导等兄弟模块只关心这个布尔）。
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn reset(&mut self) {
        *self = Self {
            generation: self.generation.wrapping_add(1),
            ..Self::default()
        };
    }
}

enum PlaylistDialog {
    Name {
        playlist: Option<Playlist>,
        songs: Vec<String>,
        input: Entity<InputState>,
    },
    Choose {
        songs: Vec<String>,
    },
    Confirm {
        edit: PlaylistEdit,
        title: String,
        message: String,
    },
}

#[derive(Clone)]
pub(super) struct SongMenuContext {
    pub songs: Arc<Vec<Song>>,
    pub index: usize,
    pub queue: bool,
    pub playlist: Option<Playlist>,
}

impl NavidromeApp {
    pub(super) fn playlist_read_error(&mut self, error: String) {
        if self.playlist_ui.dialog.is_some() {
            self.playlist_ui.error = Some(error.clone());
        }
        self.notify_error(error);
    }

    pub(super) fn load_playlist_detail(&mut self, playlist_id: String) {
        let Some(api) = self.api.clone() else { return };
        self.playlist_ui.detail_loading = true;
        self.playlist_ui.detail_request = self.playlist_ui.detail_request.wrapping_add(1);
        let request = self.playlist_ui.detail_request;
        let generation = self.playlist_ui.generation;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.playlist(&playlist_id).await.map_err(error_message);
            let _ = tx.send(Msg::PlaylistSongs {
                generation,
                request,
                playlist_id,
                result,
            });
        });
    }

    pub(super) fn start_playlist_edit(&mut self, edit: PlaylistEdit, cx: &mut Context<Self>) {
        if self.playlist_ui.busy {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        self.playlist_ui.busy = true;
        self.playlist_ui.error = None;
        self.playlist_ui.generation = self.playlist_ui.generation.wrapping_add(1);
        let generation = self.playlist_ui.generation;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.edit_playlist(&edit).await.map_err(error_message);
            let _ = tx.send(Msg::PlaylistEdited { generation, result });
        });
        cx.notify();
    }

    pub(super) fn finish_playlist_edit(
        &mut self,
        generation: u64,
        result: Result<PlaylistEdited, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if generation != self.playlist_ui.generation {
            return;
        }
        self.playlist_ui.busy = false;
        // Reads started before the write completed cannot overwrite fresh data.
        self.playlist_ui.generation = self.playlist_ui.generation.wrapping_add(1);
        match result {
            Ok(edited) => {
                // 导入向导的确认阶段也是走 start_playlist_edit 提交的；
                // 对话框同一时间只有一个，两个向导谁开着关谁的。
                let import_dialog_open = self.import_ui.stage.is_some();
                self.import_ui.reset();
                if self.playlist_ui.dialog.take().is_some() || import_dialog_open {
                    self.playlist_ui.input_subscription = None;
                    window.close_dialog(cx);
                }
                if edited.deleted {
                    self.state.playlists.retain(|p| p.id != edited.id);
                    if self
                        .state
                        .current_playlist
                        .as_ref()
                        .is_some_and(|p| p.id == edited.id)
                    {
                        self.state.current_playlist = None;
                        self.state.playlist_songs.clear();
                        if self.state.view == View::PlaylistDetail {
                            self.state.view = View::Playlists;
                        }
                    }
                    window.push_notification(Notification::success(t!("Playlist deleted")), cx);
                } else {
                    window.push_notification(Notification::success(t!("Playlist saved")), cx);
                }
            }
            Err(error) => {
                if self.playlist_ui.dialog.is_some() {
                    self.playlist_ui.error = Some(error.clone());
                }
                // 创建导入歌单失败：退回审查阶段改名字重试，不要把用户
                // 踢回粘贴链接重新来一遍。
                if self.import_ui.stage == Some(ImportStage::Review) {
                    self.import_ui.error = Some(error.clone());
                }
                self.notify_error(error);
            }
        }
        self.load_playlists();
        if let Some(playlist) = &self.state.current_playlist {
            self.load_playlist_detail(playlist.id.clone());
        } else {
            self.playlist_ui.detail_loading = false;
        }
        cx.notify();
    }

    pub(super) fn name_playlist(
        &mut self,
        playlist: Option<Playlist>,
        songs: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.playlist_ui.busy || self.api.is_none() {
            return;
        }
        let name = playlist
            .as_ref()
            .map_or("", |p| p.name.as_str())
            .to_string();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Playlist name"))
                .default_value(name)
        });
        self.playlist_ui.input_subscription =
            Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_playlist_dialog(cx);
                }
            }));
        self.playlist_ui.dialog = Some(PlaylistDialog::Name {
            playlist,
            songs,
            input: input.clone(),
        });
        self.playlist_ui.error = None;
        self.show_playlist_dialog(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(super) fn choose_playlist(
        &mut self,
        songs: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.playlist_ui.busy || self.api.is_none() || songs.is_empty() {
            return;
        }
        self.playlist_ui.dialog = Some(PlaylistDialog::Choose { songs });
        self.playlist_ui.error = None;
        self.load_playlists();
        self.show_playlist_dialog(window, cx);
    }

    fn confirm_playlist_edit(
        &mut self,
        edit: PlaylistEdit,
        title: String,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.playlist_ui.busy {
            return;
        }
        self.playlist_ui.dialog = Some(PlaylistDialog::Confirm {
            edit,
            title,
            message,
        });
        self.playlist_ui.error = None;
        self.show_playlist_dialog(window, cx);
    }

    pub(super) fn delete_playlist(
        &mut self,
        playlist: Playlist,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_playlist_edit(
            PlaylistEdit::Delete { id: playlist.id },
            t!("Delete playlist?").to_string(),
            t!(
                "Delete “%{name}” from Navidrome? The music files will be kept.",
                name = playlist.name
            )
            .to_string(),
            window,
            cx,
        );
    }

    fn show_playlist_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            // Let the menu finish dismissing before installing the dialog's
            // focus scope. Otherwise its focus restoration can swallow Enter/Escape.
            window.defer(cx, move |window, cx| {
                let render_app = app.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| this.render_playlist_dialog(dialog, cx))
                        .expect("app owns dialog")
                });
                let _ = app.update(cx, |this, cx| {
                    if let Some(PlaylistDialog::Name { input, .. }) = &this.playlist_ui.dialog {
                        input.update(cx, |input, cx| input.focus(window, cx));
                    }
                });
            });
        }
        cx.notify();
    }

    fn submit_playlist_dialog(&mut self, cx: &mut Context<Self>) {
        let edit = match &self.playlist_ui.dialog {
            Some(PlaylistDialog::Name {
                playlist,
                songs,
                input,
            }) => {
                let name = input.read(cx).value().trim().to_string();
                if name.is_empty() {
                    self.playlist_ui.error = Some(t!("Enter a playlist name.").to_string());
                    cx.notify();
                    return;
                }
                if let Some(playlist) = playlist {
                    PlaylistEdit::Rename {
                        id: playlist.id.clone(),
                        name,
                    }
                } else {
                    PlaylistEdit::Create {
                        name,
                        songs: songs.clone(),
                    }
                }
            }
            Some(PlaylistDialog::Confirm { edit, .. }) => edit.clone(),
            _ => return,
        };
        self.start_playlist_edit(edit, cx);
    }

    fn render_playlist_dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let busy = self.playlist_ui.busy;
        let mut content = v_flex().gap_3();
        let (title, action, destructive) = match &self.playlist_ui.dialog {
            Some(PlaylistDialog::Name {
                playlist,
                songs,
                input,
            }) => {
                if playlist.is_none() && !songs.is_empty() {
                    content = content.child(t!(
                        "Save %{count} tracks in their current order.",
                        count = songs.len()
                    ));
                }
                content = content.child(Input::new(input).disabled(busy).w_full());
                (
                    if playlist.is_some() {
                        t!("Rename playlist")
                    } else {
                        t!("New playlist")
                    }
                    .to_string(),
                    Some(t!("Save").to_string()),
                    false,
                )
            }
            Some(PlaylistDialog::Choose { songs }) => {
                let new_songs = songs.clone();
                content = content
                    .child(t!(
                        "Add %{count} tracks. Existing copies will be kept.",
                        count = songs.len()
                    ))
                    .child(
                        Button::new("new-playlist-from-songs")
                            .label(t!("New playlist…"))
                            .outline()
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.name_playlist(None, new_songs.clone(), window, cx)
                            })),
                    );
                let mut playlists: Vec<_> = self
                    .state
                    .playlists
                    .iter()
                    .filter(|p| can_edit(p, &self.config.username))
                    .cloned()
                    .collect();
                playlists.sort_by_key(|p| p.name.to_lowercase());
                if self.playlist_ui.list_loading {
                    content = content.child(div().text_sm().child(t!("Loading playlists…")));
                } else if playlists.is_empty() {
                    content = content.child(
                        div()
                            .text_sm()
                            .child(t!("No editable playlists yet. Create one above.")),
                    );
                }
                content = content.child(
                    v_flex()
                        .id("playlist-picker")
                        .max_h(px(260.0))
                        .overflow_y_scroll()
                        .gap_1()
                        .children(playlists.into_iter().map(|playlist| {
                            let songs = songs.clone();
                            Button::new(SharedString::from(format!(
                                "choose-playlist-{}",
                                playlist.id
                            )))
                            .label(playlist.name)
                            .ghost()
                            .w_full()
                            .justify_start()
                            .disabled(busy || self.playlist_ui.list_loading)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.start_playlist_edit(
                                        PlaylistEdit::Append {
                                            id: playlist.id.clone(),
                                            songs: songs.clone(),
                                        },
                                        cx,
                                    );
                                },
                            ))
                        })),
                );
                (t!("Add to playlist").to_string(), None, false)
            }
            Some(PlaylistDialog::Confirm {
                title,
                message,
                edit,
            }) => {
                content = content.child(div().text_sm().child(message.clone()));
                (
                    title.clone(),
                    Some(if matches!(edit, PlaylistEdit::Delete { .. }) {
                        t!("Delete").to_string()
                    } else {
                        t!("Remove").to_string()
                    }),
                    true,
                )
            }
            None => return dialog,
        };
        if let Some(error) = &self.playlist_ui.error {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        if busy {
            content = content.child(div().text_sm().child(t!("Saving…")));
        }
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-playlist-dialog")
                    .label(t!("Cancel"))
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.playlist_ui.dialog = None;
                        this.playlist_ui.input_subscription = None;
                        window.close_dialog(cx);
                        cx.notify();
                    })),
            )
            .when_some(action, |footer, action| {
                footer.child(
                    Button::new("save-playlist-dialog")
                        .label(action)
                        .primary()
                        .when(destructive, |button| button.danger())
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_playlist_dialog(cx))),
                )
            });
        let submit = cx.entity().downgrade();
        let cancel = submit.clone();
        dialog
            .title(title)
            .w(px(440.0))
            .overlay_closable(false)
            .close_button(!busy)
            .keyboard(!busy)
            .on_ok(move |_, _, cx| {
                let _ = submit.update(cx, |this, cx| this.submit_playlist_dialog(cx));
                false
            })
            .on_cancel(move |_, _, cx| {
                cancel
                    .update(cx, |this, _| !this.playlist_ui.busy)
                    .unwrap_or(true)
            })
            .on_close(cx.listener(|this, _, _, cx| {
                this.playlist_ui.dialog = None;
                this.playlist_ui.input_subscription = None;
                cx.notify();
            }))
            .child(content)
            .footer(footer)
    }

    pub(super) fn song_menu(
        &mut self,
        mut menu: PopupMenu,
        context: SongMenuContext,
        cx: &Context<Self>,
    ) -> PopupMenu {
        self.playlist_ui.menu_focus = Some(menu.focus_handle(cx));
        let song = context.songs[context.index].clone();
        let play = context.clone();
        menu = menu.item(PopupMenuItem::new(t!("Play now")).on_click(cx.listener(
            move |this, _, _, cx| {
                this.play_song_list(&play.songs, play.index);
                cx.notify();
            },
        )));
        let next = song.clone();
        menu = menu.item(PopupMenuItem::new(t!("Play next")).on_click(cx.listener(
            move |this, _, _, cx| {
                this.insert_next(next.clone());
                cx.notify();
            },
        )));
        let append = song.clone();
        menu = menu.item(PopupMenuItem::new(t!("Add to queue")).on_click(cx.listener(
            move |this, _, _, cx| {
                this.add_to_queue(append.clone());
                cx.notify();
            },
        )));
        let add = song.id.clone();
        menu = menu.separator().item(
            PopupMenuItem::new(t!("Add to playlist…"))
                .disabled(self.playlist_ui.busy || self.api.is_none())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose_playlist(vec![add.clone()], window, cx)
                })),
        );
        let key = FavoriteKey::new(FavoriteKind::Song, &song.id);
        let label = if self.state.favorite_ids.contains(&key) {
            t!("Remove from favorites")
        } else {
            t!("Add to favorites")
        };
        let favorite_id = song.id.clone();
        menu = menu.item(
            PopupMenuItem::new(label)
                .disabled(self.state.pending_favorites.contains(&key))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_favorite(FavoriteKey::new(FavoriteKind::Song, &favorite_id));
                    cx.notify();
                })),
        );
        // 与行内可点击的艺术家/专辑名一致：菜单也提供导航入口，
        // 缺少服务器 id（如整理过的杂项曲库）时禁用。
        let song_for_artist = song.clone();
        let artist_available = song.artist_id.as_deref().is_some_and(|id| !id.is_empty());
        menu = menu.item(
            PopupMenuItem::new(t!("Go to artist"))
                .disabled(!artist_available)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_song_artist(&song_for_artist);
                    cx.notify();
                })),
        );
        let song_for_album = song.clone();
        let album_available = song.album_id.as_deref().is_some_and(|id| !id.is_empty());
        menu = menu.item(
            PopupMenuItem::new(t!("Go to album"))
                .disabled(!album_available)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_song_album(&song_for_album);
                    cx.notify();
                })),
        );
        let song_for_info = song.clone();
        menu = menu
            .separator()
            .item(PopupMenuItem::new(t!("Song info…")).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.open_song_info(song_for_info.clone(), window, cx);
                },
            )));
        if let Some(playlist) = context.playlist {
            let disabled = self.playlist_ui.busy
                || self.playlist_ui.detail_loading
                || !can_edit(&playlist, &self.config.username);
            let expected = song_ids(&context.songs);
            menu = menu.separator();
            for (label, to) in [
                (t!("Move up"), context.index.checked_sub(1)),
                (
                    t!("Move down"),
                    (context.index + 1 < expected.len()).then_some(context.index + 1),
                ),
            ] {
                let edit = PlaylistEdit::Move {
                    id: playlist.id.clone(),
                    expected: expected.clone(),
                    from: context.index,
                    to: to.unwrap_or(context.index),
                };
                menu = menu.item(
                    PopupMenuItem::new(label)
                        .disabled(disabled || to.is_none())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_playlist_edit(edit.clone(), cx)
                        })),
                );
            }
            let edit = PlaylistEdit::Remove {
                id: playlist.id,
                expected,
                index: context.index,
            };
            let message = t!(
                "Remove “%{title}” (track %{number}) from “%{playlist}”?",
                title = song.title,
                number = context.index + 1,
                playlist = playlist.name
            )
            .to_string();
            menu = menu.item(
                PopupMenuItem::new(t!("Remove from playlist…"))
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_playlist_edit(
                            edit.clone(),
                            t!("Remove track?").to_string(),
                            message.clone(),
                            window,
                            cx,
                        )
                    })),
            );
        } else if context.queue {
            let expected = song_ids(&context.songs);
            menu = menu
                .separator()
                .item(
                    PopupMenuItem::new(t!("Remove from queue")).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if song_ids(&this.state.queue) == expected {
                                this.remove_from_queue(context.index);
                            }
                            cx.notify();
                        },
                    )),
                );
        }
        menu
    }
}
