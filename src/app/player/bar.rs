//! Player bar layout, current-track information and accessory controls.

use crate::app::library::navigation::{album_for_song, artist_for_song};
use crate::app::playback::display::playback_technical_info;
use crate::app::prelude::*;
use crate::app::shortcuts::bindings::shortcut_display;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

impl NavidromeApp {
    pub(in crate::app) fn render_player(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let playback = self.playback_for_display();
        let brand = palette(cx).brand;
        let viewport_width = f32::from(window.viewport_size().width);
        let layout = self.shell_layout(window);
        let compact_player = layout.compact;
        // Keep the technical chip on wider windows; details are also available
        // on the now-playing page.
        let show_technical_chip = viewport_width >= 1080.0;
        let cover_tint = self.now_playing_cover_palette().map(|(base, _)| base);
        translucent_surface(
            h_flex()
                .flex_none()
                .h(px(if compact_player {
                    tokens::PLAYER_COMPACT_HEIGHT
                } else {
                    tokens::PLAYER_HEIGHT
                }))
                .mx(px(layout.player_margin))
                .mb(px(layout.player_margin))
                .when(compact_player, |this| this.px_2())
                .when(!compact_player, |this| this.px_3())
                .gap(if compact_player { px(8.0) } else { px(16.0) })
                .items_center()
                .justify_between()
                .occlude(),
            tokens::radius::LARGE,
            cover_tint,
            cx,
        )
        .child(self.render_player_track(compact_player, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .items_center()
                .child(self.render_player_transport(&playback, compact_player, cx))
                .child(self.render_player_progress(&playback, brand, cx)),
        )
        .child(self.render_player_actions(compact_player, show_technical_chip, brand, cx))
        .into_any_element()
    }

    fn render_player_track(
        &self,
        compact_player: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let song_info_width = if compact_player { 200.0 } else { 260.0 };
        let player_cover_size = if compact_player { 40.0 } else { 48.0 };
        if let Some(song) = &self.state.now_playing {
            // 悬停提示显示完整标题/艺人（长标题被截断时也能看全文），
            // 并说明点击可打开正在播放页。
            let tooltip_title = song
                .bilibili_subtitle()
                .map(|subtitle| format!("{}\n{subtitle}", song.title))
                .unwrap_or_else(|| song.title.clone());
            let tooltip_artist = song
                .bilibili_part
                .as_ref()
                .map(|part| t!("Uploader: %{name}", name = part.uploader).to_string())
                .unwrap_or_else(|| song.artist.clone());
            let tooltip_text = if tooltip_artist.trim().is_empty() {
                t!("%{title}\nOpen now playing", title = tooltip_title).to_string()
            } else {
                t!(
                    "%{title} — %{artist}\nOpen now playing",
                    title = tooltip_title,
                    artist = tooltip_artist
                )
                .to_string()
            };
            h_flex()
                .id("now-playing-info")
                .w(px(song_info_width))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap(if compact_player { px(8.0) } else { px(12.0) })
                .p_1()
                .items_center()
                .cursor_pointer()
                .rounded(px(tokens::radius::COVER))
                .hover(|style| style.bg(palette(cx).sunken))
                .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
                .child(self.render_song_cover_styled(
                    song,
                    player_cover_size,
                    tokens::radius::COVER,
                    cover_shadow(false, cx),
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(tokens::BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(song.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(match song.bilibili_subtitle() {
                                    Some(subtitle) => {
                                        div().truncate().child(subtitle).into_any_element()
                                    }
                                    None => self.song_artist_link(
                                        SharedString::from(format!(
                                            "player-bar-artist-{}",
                                            song.id
                                        )),
                                        song,
                                        cx,
                                    ),
                                }),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_now_playing();
                    cx.notify();
                }))
                .context_menu({
                    let app = cx.entity().downgrade();
                    move |menu, _, cx| {
                        app.update(cx, |this, cx| this.now_playing_context_menu(menu, cx))
                            .expect("app owns now playing menu")
                    }
                })
                .into_any_element()
        } else {
            h_flex()
                .w(px(song_info_width))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap(if compact_player { px(8.0) } else { px(12.0) })
                .p_1()
                .items_center()
                .child(self.render_cover(None, player_cover_size, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(tokens::BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(t!("No track playing")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(t!("Choose a song from your library.")),
                        ),
                )
                .into_any_element()
        }
    }

    fn render_player_actions(
        &self,
        compact_player: bool,
        show_technical_chip: bool,
        accent: Hsla,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let right_controls_width = if compact_player { 180.0 } else { 260.0 };
        let queue_open = self.state.queue_drawer_open;
        let technical_info = self
            .state
            .now_playing
            .as_ref()
            .zip(self.state.now_playing_quality)
            .map(|(song, quality)| {
                if song.bilibili_part.is_some() {
                    crate::app::playback::display::bilibili_technical_info(
                        self.state.now_playing_audio_quality.as_ref(),
                    )
                } else {
                    playback_technical_info(song, quality)
                }
            });
        h_flex()
            .w(px(right_controls_width))
            .h(px(40.0))
            .min_w_0()
            .flex_shrink_0()
            .justify_end()
            .items_center()
            .gap(if compact_player { px(4.0) } else { px(8.0) })
            .when(compact_player, |this| this.pl_2())
            .when(!compact_player, |this| this.pl_3())
            .when_some(
                technical_info.filter(|_| show_technical_chip),
                |this, info| {
                    let tooltip = info.tooltip.clone();
                    this.child(
                        h_flex()
                            .id("player-technical-info")
                            .min_w_0()
                            .gap_0p5()
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .child(technical_info_chip("player-format", info.format, cx)),
                    )
                },
            )
            .when_some(self.state.now_playing.as_ref(), |this, song| {
                this.child(self.favorite_button_sized(
                    FavoriteKind::Song,
                    &song.id,
                    tokens::ICON_BUTTON_MD,
                    cx,
                ))
            })
            .child(
                toggle_icon_button(
                    "player-overlay-lyrics",
                    AppIcon::Translation,
                    if self.overlay_lyrics_open() {
                        if self.config.lyrics.overlay.locked {
                            t!("Unlock desktop lyrics")
                        } else {
                            t!("Close desktop lyrics")
                        }
                    } else {
                        t!("Desktop lyrics")
                    },
                    self.overlay_lyrics_open(),
                    tokens::ICON_BUTTON_MD,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.overlay_lyrics_open() && this.config.lyrics.overlay.locked {
                        this.set_overlay_lyrics_locked(false, cx);
                    } else {
                        this.toggle_overlay_lyrics(cx);
                    }
                })),
            )
            .child(self.render_eq_popover(cx))
            .child(
                toggle_icon_button(
                    "player-queue",
                    AppIcon::Queue,
                    t!(
                        "Queue (%{shortcut})",
                        shortcut = shortcut_display(
                            self.config.shortcuts.binding(ShortcutAction::ToggleQueue)
                        )
                    ),
                    queue_open,
                    tokens::ICON_BUTTON_MD,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.state.settings.close();
                    this.state.queue_drawer_open = !this.state.queue_drawer_open;
                    if !this.state.queue_drawer_open {
                        this.clear_song_multi();
                    }
                    cx.notify();
                })),
            )
            .child(self.render_volume_control(accent, cx))
            .into_any_element()
    }

    pub(in crate::app) fn now_playing_context_menu(
        &self,
        mut menu: PopupMenu,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let Some(song) = self.state.now_playing.clone() else {
            return menu;
        };
        let is_bilibili = song
            .source
            .as_ref()
            .is_some_and(|source| source.provider == crate::source::ProviderKind::Bilibili);
        let browsing_owner = self.browsing_owns_song(&song);
        let capabilities = self
            .api
            .as_ref()
            .map(MusicSource::capabilities)
            .unwrap_or_default();

        if !is_bilibili {
            let radio_seed = song.clone();
            let radio_loading = self.song_radio_loading(&song.id);
            menu = menu.item(
                PopupMenuItem::new(if radio_loading {
                    t!("Building song radio…")
                } else {
                    t!("Start song radio")
                })
                .disabled(
                    self.source_for_song(&song)
                        .is_none_or(|api| !api.capabilities().recommendations)
                        || radio_loading,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.start_song_radio(radio_seed.clone());
                    cx.notify();
                })),
            );
            let add_song = song.clone();
            menu = menu.item(
                PopupMenuItem::new(t!("Add to playlist…"))
                    .disabled(
                        self.state.playlists.editing()
                            || !browsing_owner
                            || !capabilities.playlist_write,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.browsing_owns_song(&add_song) {
                            this.choose_playlist(vec![add_song.id.clone()], window, cx);
                        }
                    })),
            );
            let key = FavoriteKey::new(FavoriteKind::Song, &song.id);
            let fav_label = if self.state.favorites.is_starred(&key) {
                t!("Remove from favorites")
            } else {
                t!("Add to favorites")
            };
            let fav_song = song.clone();
            menu = menu.item(
                PopupMenuItem::new(fav_label)
                    .disabled(
                        self.state.favorites.is_pending(&key)
                            || !browsing_owner
                            || !capabilities.favorite_write,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.browsing_owns_song(&fav_song) {
                            this.toggle_favorite(FavoriteKey::new(
                                FavoriteKind::Song,
                                &fav_song.id,
                            ));
                        }
                        cx.notify();
                    })),
            );
            let song_for_artist = song.clone();
            let artist_available =
                browsing_owner && capabilities.library && artist_for_song(&song).is_some();
            menu = menu.separator().item(
                PopupMenuItem::new(t!("Go to artist"))
                    .disabled(!artist_available)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_song_artist(&song_for_artist);
                        cx.notify();
                    })),
            );
            let song_for_album = song.clone();
            let album_available =
                browsing_owner && capabilities.library && album_for_song(&song).is_some();
            menu = menu.item(
                PopupMenuItem::new(t!("Go to album"))
                    .disabled(!album_available)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_song_album(&song_for_album);
                        cx.notify();
                    })),
            );
        }

        let copy_summary = if song.artist.trim().is_empty() {
            song.title.clone()
        } else {
            format!("{} - {}", song.title, song.artist)
        };
        let song_for_info = song;
        menu.separator()
            .item(
                PopupMenuItem::new(t!("Copy title & artist")).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.copy_text_to_clipboard(copy_summary.clone(), cx);
                    },
                )),
            )
            .item(PopupMenuItem::new(t!("Song info…")).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.open_song_info(song_for_info.clone(), window, cx);
                },
            )))
    }
}

fn technical_info_chip(
    id: &'static str,
    label: String,
    cx: &Context<NavidromeApp>,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    div()
        .id(id)
        .h(px(20.0))
        .max_w(px(68.0))
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(p.brand_soft)
        .border_1()
        .border_color(p.brand.opacity(0.18))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.brand)
        .truncate()
        .child(label)
        .into_any_element()
}
