//! Bilibili home and folder cards with video-shaped artwork and local filtering.

use crate::app::home::greeting::current_greeting;
use crate::app::prelude::*;
use crate::app::style::cover_tint::{cover_tint, tint_background, TintSurface};
use crate::app::widgets::shelf::{cover_grid, shelf_section};
use gpui_kit::Focusable;

impl NavidromeApp {
    pub(in crate::app) fn render_bilibili_thumbnail_styled(
        &self,
        cover: Option<Arc<GpuiImage>>,
        width: f32,
        radius: f32,
        hovered: bool,
        cx: &App,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let height = width * 9.0 / 16.0;
        let shadows = cover_shadow(hovered, cx);
        if let Some(cover) = cover {
            self.mark_cover_image_used(&cover);
            crate::app::covers::thumbnails::FilteredVideoCover {
                source: cover,
                cache: self.cover_thumbnails.clone(),
                width,
                radius,
                shadows,
            }
            .into_any_element()
        } else {
            let icon_size = (height * 0.34).clamp(14.0, 32.0);
            div()
                .w(px(width))
                .h(px(height))
                .flex_none()
                .rounded(px(radius))
                .bg(p.sunken)
                .shadow(shadows)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(AppIcon::Video)
                        .with_size(px(icon_size))
                        .text_color(p.text_muted.opacity(0.65)),
                )
                .into_any_element()
        }
    }

    fn bilibili_folder_card(
        &self,
        folder: &Playlist,
        width: f32,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let target = folder.clone();
        let source = self.api.as_ref().map(|api| api.id().clone());
        let hover_key = format!("bilibili-folder-{}", folder.id);
        let hovered = self.state.hovered_item.as_deref() == Some(hover_key.as_str());
        let hover_key_for_cb = hover_key.clone();
        v_flex()
            .id(SharedString::from(hover_key))
            .min_w_0()
            .gap_2()
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovering, _, cx| {
                this.set_hovered(&hover_key_for_cb, *hovering, cx);
            }))
            .child(
                div()
                    .relative()
                    .w(px(width))
                    .h(px(width * 9.0 / 16.0))
                    .flex_none()
                    .child(
                        self.render_bilibili_thumbnail_styled(
                            folder
                                .cover_art
                                .as_deref()
                                .and_then(|id| self.browsing_cover_image(id))
                                .cloned(),
                            width,
                            tokens::radius::COVER,
                            hovered,
                            cx,
                        ),
                    )
                    .when_some(folder.song_count, |this, count| {
                        this.child(video_corner_badge(
                            t!("%{count} videos", count = count).into(),
                        ))
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(tokens::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(folder.name.clone()),
                    )
                    .when_some(folder.song_count, |this, count| {
                        this.child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(t!("%{count} videos", count = count)),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.api.as_ref().map(|api| api.id()) == source.as_ref() {
                    this.open_playlist(target.clone());
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    fn render_bilibili_folder_shortcuts(
        &self,
        folders: &[&Playlist],
        visited: bool,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let error = self.state.playlists.list_error();
        let action_label = if error.is_some() {
            t!("Retry")
        } else {
            t!("Browse favorite folders")
        };
        let card = raised_surface(
            v_flex()
                .w_full()
                .min_w_0()
                .min_h(px(if compact {
                    tokens::HERO_CARD_COMPACT_HEIGHT
                } else {
                    tokens::HERO_CARD_HEIGHT
                }))
                .when(compact, |this| this.p_4().gap_2())
                .when(!compact, |this| this.p_5().gap_3()),
            tokens::radius::LARGE,
            cx,
        )
        .id("bilibili-folder-shortcuts")
        .debug_selector(|| "bilibili-folder-shortcuts".into())
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_2()
                .child(
                    section_title(
                        if visited {
                            t!("Recently opened folders")
                        } else {
                            t!("Favorite folders")
                        },
                        cx,
                    )
                    .flex_1()
                    .min_w_0()
                    .truncate(),
                )
                .child(
                    Button::new("bilibili-all-folders")
                        .icon(if error.is_some() {
                            AppIcon::Refresh
                        } else {
                            AppIcon::ChevronRight
                        })
                        .accessibility_label(action_label.clone())
                        .tooltip(action_label)
                        .ghost()
                        .small()
                        .w(px(30.0))
                        .h(px(30.0))
                        .disabled(self.api.is_none())
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.state.playlists.list_error().is_some() {
                                this.load_playlists();
                            } else {
                                this.navigate_to(View::Playlists);
                            }
                            cx.notify();
                        })),
                ),
        );
        if let Some(error) = error {
            return card
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.to_string()),
                )
                .into_any_element();
        }
        if folders.is_empty() {
            return card
                .child(
                    h_flex()
                        .flex_1()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .text_color(p.text_secondary)
                        .child(
                            Icon::new(AppIcon::Playlists)
                                .with_size(px(16.0))
                                .text_color(p.text_muted),
                        )
                        .child(if self.api.is_none() {
                            self.state.status.clone().into()
                        } else if self.state.playlists.list_loading() {
                            t!("Loading favorites…")
                        } else {
                            t!("No Bilibili favorites yet")
                        }),
                )
                .into_any_element();
        }
        let max_rows = if compact { 2 } else { 3 };
        card.child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_1()
                .children(folders.iter().take(max_rows).map(|folder| {
                    let target = (*folder).clone();
                    let source = self.api.as_ref().map(|api| api.id().clone());
                    let thumb = folder
                        .cover_art
                        .as_deref()
                        .and_then(|id| self.browsing_cover_image(id))
                        .cloned();
                    h_flex()
                        .id(SharedString::from(format!(
                            "bilibili-folder-shortcut-{}",
                            folder.id
                        )))
                        .w_full()
                        .min_w_0()
                        .h(px(34.0))
                        .px_2()
                        .items_center()
                        .gap_2p5()
                        .rounded(px(tokens::radius::CONTROL))
                        .cursor_pointer()
                        .hover(|style| style.bg(p.sunken))
                        .child(if thumb.is_some() {
                            self.render_bilibili_thumbnail_styled(thumb, 44.0, 5.0, false, cx)
                        } else {
                            div()
                                .w(px(44.0))
                                .h(px(25.0))
                                .flex_none()
                                .rounded(px(5.0))
                                .bg(p.sunken)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(AppIcon::Playlists)
                                        .with_size(px(13.0))
                                        .text_color(p.text_muted),
                                )
                                .into_any_element()
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(tokens::BODY))
                                .truncate()
                                .child(folder.name.clone()),
                        )
                        .when_some(folder.song_count, |this, count| {
                            this.child(
                                div()
                                    .flex_none()
                                    .text_size(px(tokens::CAPTION))
                                    .text_color(p.text_muted)
                                    .child(t!("%{count} videos", count = count)),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.api.as_ref().map(|api| api.id()) == source.as_ref() {
                                this.open_playlist(target.clone());
                                cx.notify();
                            }
                        }))
                        .into_any_element()
                })),
        )
        .into_any_element()
    }

    pub(in crate::app) fn render_bilibili_home(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let layout = self.shell_layout(window);
        let compact = layout.compact;
        let columns = self.video_content_columns(window);
        let source = self.api.as_ref().map(|api| api.id());
        let current = self
            .state
            .now_playing
            .as_ref()
            .filter(|song| song.source.as_ref() == source && source.is_some());
        let recent = self.bilibili_recent_tracks();
        let continue_song = current.or_else(|| recent.first().copied());
        let hero_cover_width = if compact {
            tokens::VIDEO_HERO_COVER_COMPACT
        } else {
            tokens::VIDEO_HERO_COVER
        };
        let play_size = if compact {
            tokens::PLAY_BUTTON_COMPACT
        } else {
            tokens::PLAY_BUTTON
        };
        let continue_card = if let Some(song) = continue_song {
            let target = song.clone();
            let playback = self.audio.state();
            let is_playing = current.is_some() && playback.active && !playback.paused;
            let tint = if current.is_some() {
                cover_tint(
                    TintSurface::HomeFeature,
                    self.now_playing_cover_palette(),
                    cx.theme().background,
                    cx.theme().is_dark(),
                )
            } else {
                self.tint_for_cover(TintSurface::HomeFeature, song.cover_art.as_deref(), cx)
            };
            compact_continue_card(
                song.title.clone().into(),
                song.artist.clone().into(),
                self.render_bilibili_thumbnail_styled(
                    self.song_cover_image(song).cloned(),
                    hero_cover_width,
                    tokens::radius::COVER,
                    false,
                    cx,
                ),
                round_play_button(
                    "bilibili-continue",
                    if is_playing {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    },
                    if is_playing {
                        t!("Pause")
                    } else if current.is_some() {
                        t!("Continue listening")
                    } else {
                        t!("Play")
                    },
                    play_size,
                )
                .debug_selector(|| "bilibili-continue-action".into())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.browsing_owns_song(&target) {
                        return;
                    }
                    if this
                        .state
                        .now_playing
                        .as_ref()
                        .is_some_and(|song| song.media_key() == target.media_key())
                    {
                        let playback = this.audio.state();
                        if playback.paused || !playback.active {
                            this.toggle_playback();
                        } else {
                            this.open_now_playing();
                        }
                    } else {
                        this.replace_queue_and_play(vec![target.clone()], 0, QueueOrigin::Manual);
                    }
                    cx.notify();
                })),
                tint,
                compact,
                cx,
            )
        } else {
            compact_continue_card(
                t!("Start with a favorite folder.").into(),
                t!("Your Bilibili listening, in one place.").into(),
                self.render_bilibili_thumbnail_styled(
                    None,
                    hero_cover_width,
                    tokens::radius::COVER,
                    false,
                    cx,
                ),
                Button::new("bilibili-browse-folders")
                    .icon(AppIcon::ChevronRight)
                    .tooltip(t!("Browse favorite folders"))
                    .accessibility_label(t!("Browse favorite folders"))
                    .secondary()
                    .rounded(px(tokens::radius::FULL))
                    .w(px(play_size))
                    .h(px(play_size))
                    .disabled(self.api.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate_to(View::Playlists);
                        cx.notify();
                    })),
                None,
                compact,
                cx,
            )
        };
        let visited = self
            .state
            .bilibili
            .visited
            .iter()
            .filter_map(|id| {
                self.state
                    .playlists
                    .items()
                    .iter()
                    .find(|folder| &folder.id == id)
            })
            .collect::<Vec<_>>();
        let has_visited = !visited.is_empty();
        let folders = if !has_visited {
            self.state
                .playlists
                .items()
                .iter()
                .take(6)
                .collect::<Vec<_>>()
        } else {
            visited
        };

        let recent_content = if self.state.bilibili.history_loading() && recent.is_empty() {
            v_flex()
                .gap_3()
                .child(
                    div()
                        .text_size(px(tokens::META))
                        .text_color(p.text_secondary)
                        .child(t!("Loading listening history…")),
                )
                .child(self.render_video_skeleton_grid(window, cx))
                .into_any_element()
        } else if self.state.bilibili.history_error().is_some() {
            h_flex()
                .py_6()
                .items_center()
                .gap_3()
                .child(
                    Button::new("retry-bilibili-history")
                        .label(t!("Retry listening history"))
                        .outline()
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_bilibili_home();
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else if recent.is_empty() && self.api.is_some() {
            bilibili_empty_state(
                AppIcon::Video,
                t!("No listening history yet").into(),
                t!("Start with a favorite folder.").into(),
                cx,
            )
        } else {
            cover_grid(
                columns,
                recent
                    .iter()
                    .map(|song| {
                        let target_card = (*song).clone();
                        let target_play = (*song).clone();
                        let hover_key = format!("bilibili-recent-card-{}", song.id);
                        let hovered =
                            self.state.hovered_item.as_deref() == Some(hover_key.as_str());
                        let hover_key_for_cb = hover_key.clone();
                        v_flex()
                            .min_w_0()
                            .gap_2()
                            .cursor_pointer()
                            .id(SharedString::from(hover_key))
                            .on_hover(cx.listener(move |this, hovering, _, cx| {
                                this.set_hovered(&hover_key_for_cb, *hovering, cx);
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.browsing_owns_song(&target_card) {
                                    this.replace_queue_and_play(
                                        vec![target_card.clone()],
                                        0,
                                        QueueOrigin::Manual,
                                    );
                                    cx.notify();
                                }
                            }))
                            .child(recent_play_cover(
                                format!("bilibili-recent-cover-{}", song.id).into(),
                                columns.width,
                                self.render_bilibili_thumbnail_styled(
                                    self.song_cover_image(song).cloned(),
                                    columns.width,
                                    tokens::radius::COVER,
                                    hovered,
                                    cx,
                                ),
                                hovered.then(|| {
                                    round_play_button(
                                        SharedString::from(format!("bilibili-recent-{}", song.id)),
                                        PlayerIcon::Play,
                                        t!("Play"),
                                        tokens::HOVER_PLAY_BUTTON,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            if this.browsing_owns_song(&target_play) {
                                                this.replace_queue_and_play(
                                                    vec![target_play.clone()],
                                                    0,
                                                    QueueOrigin::Manual,
                                                );
                                                cx.notify();
                                            }
                                        },
                                    ))
                                }),
                                (!hovered && song.duration.is_some())
                                    .then(|| format_duration(song.duration).into()),
                            ))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(px(tokens::BODY))
                                            .font_weight(FontWeight::MEDIUM)
                                            .truncate()
                                            .child(song.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(tokens::META))
                                            .text_color(p.text_secondary)
                                            .truncate()
                                            .child(song.artist.clone()),
                                    ),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
        };

        v_flex()
            .gap(px(tokens::SECTION_GAP))
            .child(
                v_flex()
                    .gap_6()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(serif_heading(current_greeting(), tokens::PAGE_TITLE, cx))
                            .child(
                                div()
                                    .text_size(px(tokens::META))
                                    .text_color(p.text_secondary)
                                    .child(t!("Your Bilibili listening, in one place.")),
                            ),
                    )
                    .when(self.api.is_none(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(p.text_secondary)
                                .child(self.state.status.clone()),
                        )
                    })
                    .child(bilibili_home_lead_row(
                        layout.content_width,
                        continue_card,
                        self.render_bilibili_folder_shortcuts(&folders, has_visited, compact, cx),
                    )),
            )
            .child(shelf_section(
                section_title(t!("Recently played"), cx).into_any_element(),
                recent_content,
            ))
            .when_some(self.state.playlists.list_error(), |this, error| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.to_string()),
                )
            })
            .into_any_element()
    }

    pub(in crate::app) fn render_bilibili_folders(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let layout = self.shell_layout(window);
        let columns = self.video_content_columns(window);
        let folders = super::state::matching_folders(
            self.state.playlists.items(),
            &self.state.bilibili.folder_query,
        );
        let limit = self.state.bilibili.folder_limit.max(50);
        let focused = self
            .bilibili_folder_search
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let filter_bar = h_flex()
            .when(layout.compact, |this| this.w_full())
            .when(!layout.compact, |this| this.w(px(280.0)))
            .h(px(36.0))
            .px_3()
            .items_center()
            .rounded_full()
            .bg(p.sunken)
            .border_1()
            .border_color(if focused {
                cx.theme().ring
            } else {
                p.border.opacity(0.6)
            })
            .child(
                Input::new(&self.bilibili_folder_search)
                    .aria_label(t!("Find a favorite folder…"))
                    .appearance(false)
                    .w_full()
                    .min_w_0()
                    .px_0()
                    .text_size(px(tokens::META))
                    .prefix(Icon::new(KitIcon::Search).small().text_color(p.text_muted)),
            )
            .when(!self.state.bilibili.folder_query.is_empty(), |this| {
                this.child(
                    Button::new("clear-bilibili-folder-search")
                        .icon(AppIcon::Close)
                        .tooltip(t!("Clear folder search"))
                        .ghost()
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.bilibili_folder_search
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            this.state.bilibili.set_folder_query(String::new());
                            cx.notify();
                        })),
                )
            });

        v_flex()
            .gap_6()
            .child(
                h_flex()
                    .w_full()
                    .items_end()
                    .gap_3()
                    .flex_wrap()
                    .child(div().flex_1().min_w(px(220.0)).child(self.page_header(
                        t!("Favorite folders"),
                        t!(
                            "%{count} favorite folders",
                            count = self.state.playlists.items().len()
                        ),
                        cx,
                    )))
                    .child(
                        h_flex()
                            .when(layout.compact, |this| this.w_full())
                            .items_center()
                            .gap_2()
                            .child(filter_bar)
                            .child(self.header_refresh_button(
                                "refresh-bilibili-folders",
                                t!("Refresh favorites"),
                                self.state.playlists.list_loading(),
                                cx,
                                |this, _| this.load_playlists(),
                            )),
                    ),
            )
            .when_some(self.state.playlists.list_error(), |this, error| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.to_string()),
                )
            })
            .when(self.state.bilibili.cover_failures > 0, |this| {
                this.child(div().text_xs().text_color(p.text_secondary).child(t!(
                    "Some folder covers could not be loaded. Refresh to retry."
                )))
            })
            .when(
                self.state.playlists.list_loading() && self.state.playlists.items().is_empty(),
                |this| this.child(self.render_video_skeleton_grid(window, cx)),
            )
            .when(!folders.is_empty(), |this| {
                this.child(cover_grid(
                    columns,
                    folders
                        .iter()
                        .take(limit)
                        .map(|folder| self.bilibili_folder_card(folder, columns.width, cx))
                        .collect::<Vec<_>>(),
                ))
            })
            .when(
                !self.state.playlists.list_loading()
                    && self.state.playlists.list_error().is_none()
                    && folders.is_empty(),
                |this| {
                    this.child(bilibili_empty_state(
                        AppIcon::Playlists,
                        if self.state.playlists.items().is_empty() {
                            t!("No Bilibili favorites yet").into()
                        } else {
                            t!("No matching favorite folders.").into()
                        },
                        if self.state.playlists.items().is_empty() {
                            t!("Save videos to your favorites in Bilibili, then refresh here.")
                                .into()
                        } else {
                            "".into()
                        },
                        cx,
                    ))
                },
            )
            .when(folders.len() > limit, |this| {
                this.child(
                    div().pt_2().flex().justify_center().child(
                        capsule_button("more-bilibili-folders")
                            .label(t!("Show more folders"))
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.bilibili.show_more_folders();
                                cx.notify();
                            })),
                    ),
                )
            })
            .into_any_element()
    }
}

pub(super) fn video_corner_badge(label: SharedString) -> gpui_kit::AnyElement {
    div()
        .absolute()
        .right_2()
        .bottom_2()
        .px_1p5()
        .py_0p5()
        .rounded(px(6.0))
        .bg(hsla(0.0, 0.0, 0.06, 0.72))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(0.0, 0.0, 0.98, 1.0))
        .child(label)
        .into_any_element()
}

pub(super) fn bilibili_empty_state(
    icon: AppIcon,
    title: SharedString,
    subtitle: SharedString,
    cx: &App,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    v_flex()
        .w_full()
        .py_12()
        .items_center()
        .gap_2()
        .child(
            Icon::new(icon)
                .with_size(px(44.0))
                .text_color(p.text_muted.opacity(0.45)),
        )
        .child(
            div()
                .text_size(px(tokens::BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text)
                .child(title),
        )
        .when(!subtitle.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(tokens::META))
                    .text_color(p.text_secondary)
                    .child(subtitle),
            )
        })
        .into_any_element()
}

fn bilibili_home_lead_row(
    width: f32,
    continue_card: gpui_kit::AnyElement,
    folders_card: gpui_kit::AnyElement,
) -> gpui_kit::AnyElement {
    div()
        .id("bilibili-home-lead-row")
        .debug_selector(|| "bilibili-home-lead-row".into())
        .w_full()
        .grid()
        .grid_cols(if width >= tokens::HOME_FEATURE_PAIR_MIN_WIDTH {
            2
        } else {
            1
        })
        .gap(px(crate::app::style::grid::GRID_GAP))
        .child(continue_card)
        .child(folders_card)
        .into_any_element()
}

fn compact_continue_card(
    title: SharedString,
    artist: SharedString,
    thumbnail: gpui_kit::AnyElement,
    action: Button,
    tint: Option<(Hsla, Hsla)>,
    compact: bool,
    cx: &App,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    let tooltip = title.clone();
    let card = h_flex()
        .id("bilibili-continue-card")
        .debug_selector(|| "bilibili-continue-card".into())
        .w_full()
        .min_w_0()
        .min_h(px(if compact {
            tokens::HERO_CARD_COMPACT_HEIGHT
        } else {
            tokens::HERO_CARD_HEIGHT
        }))
        .when(compact, |this| this.p_4().gap_4())
        .when(!compact, |this| this.p_5().gap_5())
        .items_center()
        .rounded(px(tokens::radius::LARGE))
        .bg(tint_background(tint, p.sunken, 118.0));

    card.child(thumbnail)
        .child(
            h_flex()
                .id("bilibili-continue-main")
                .debug_selector(|| "bilibili-continue-main".into())
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_3()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_secondary)
                                .child(t!("Continue listening")),
                        )
                        .child(
                            serif_heading(
                                title,
                                if compact {
                                    tokens::SECTION_TITLE
                                } else {
                                    tokens::CARD_TITLE_COMPACT
                                },
                                cx,
                            )
                            .id("bilibili-continue-title")
                            .debug_selector(|| "bilibili-continue-title".into())
                            .truncate()
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            }),
                        )
                        .when(!artist.is_empty(), |this| {
                            this.child(
                                div()
                                    .text_size(px(tokens::META))
                                    .text_color(p.text_secondary)
                                    .truncate()
                                    .child(artist),
                            )
                        }),
                )
                .child(action.flex_none()),
        )
        .into_any_element()
}

pub(super) fn recent_play_cover(
    id: SharedString,
    width: f32,
    thumbnail: gpui_kit::AnyElement,
    action: Option<Button>,
    badge: Option<SharedString>,
) -> gpui_kit::AnyElement {
    let selector = id.clone();
    div()
        .id(id)
        .debug_selector(move || selector.to_string())
        .relative()
        .w(px(width))
        .h(px(width * 9.0 / 16.0))
        .flex_none()
        .child(thumbnail)
        .when_some(badge, |this, label| this.child(video_corner_badge(label)))
        .when_some(action, |this, action| {
            this.child(
                div()
                    .absolute()
                    .right_2()
                    .bottom_2()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(action),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests;
