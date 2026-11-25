//! Bilibili home and folder cards with video-shaped artwork and local filtering.

use crate::app::prelude::*;
use crate::app::widgets::shelf::cover_grid;
use gpui_kit::Focusable;

impl NavidromeApp {
    pub(super) fn render_bilibili_thumbnail(
        &self,
        cover: Option<Arc<GpuiImage>>,
        width: f32,
        cx: &App,
    ) -> gpui_kit::AnyElement {
        let frame = div()
            .w(px(width))
            .h(px(width * 9.0 / 16.0))
            .flex_none()
            .rounded(px(tokens::radius::COVER))
            .overflow_hidden();
        if let Some(cover) = cover {
            self.mark_cover_image_used(&cover);
            frame
                .child(crate::app::covers::thumbnails::FilteredVideoCover {
                    source: cover,
                    cache: self.cover_thumbnails.clone(),
                    width,
                })
                .into_any_element()
        } else {
            frame
                .bg(palette(cx).sunken)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(AppIcon::Video)
                        .with_size(px(32.0))
                        .text_color(palette(cx).text_muted),
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
        let target = folder.clone();
        let source = self.api.as_ref().map(|api| api.id().clone());
        v_flex()
            .id(SharedString::from(format!("bilibili-folder-{}", folder.id)))
            .min_w_0()
            .gap_2()
            .cursor_pointer()
            .child(
                self.render_bilibili_thumbnail(
                    folder
                        .cover_art
                        .as_deref()
                        .and_then(|id| self.browsing_cover_image(id))
                        .cloned(),
                    width,
                    cx,
                ),
            )
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
                        .text_color(palette(cx).text_secondary)
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
    }

    fn render_bilibili_folder_shortcuts(
        &self,
        folders: &[&Playlist],
        visited: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
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
                .min_h(px(tokens::HERO_CARD_COMPACT_HEIGHT))
                .p_4()
                .gap_2(),
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
                        .w(px(32.0))
                        .h(px(32.0))
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
                    div()
                        .text_sm()
                        .text_color(palette(cx).text_secondary)
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
        card.children(folders.iter().take(2).map(|folder| {
            let target = (*folder).clone();
            let source = self.api.as_ref().map(|api| api.id().clone());
            h_flex()
                .id(SharedString::from(format!(
                    "bilibili-folder-shortcut-{}",
                    folder.id
                )))
                .w_full()
                .min_w_0()
                .h(px(28.0))
                .items_center()
                .gap_2()
                .rounded(px(tokens::radius::CONTROL))
                .cursor_pointer()
                .hover(|style| style.bg(palette(cx).sunken))
                .child(Icon::new(AppIcon::Playlists).with_size(px(16.0)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .truncate()
                        .child(folder.name.clone()),
                )
                .when_some(folder.song_count, |this, count| {
                    this.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(palette(cx).text_muted)
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
        }))
        .into_any_element()
    }

    pub(in crate::app) fn render_bilibili_home(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let source = self.api.as_ref().map(|api| api.id());
        let current = self
            .state
            .now_playing
            .as_ref()
            .filter(|song| song.source.as_ref() == source && source.is_some());
        let recent = self.bilibili_recent_tracks();
        let continue_song = current.or_else(|| recent.first().copied());
        let continue_card = if let Some(song) = continue_song {
            let target = song.clone();
            compact_continue_card(
                song.title.clone().into(),
                song.artist.clone().into(),
                self.render_bilibili_thumbnail(
                    self.song_cover_image(song).cloned(),
                    tokens::HERO_COVER_COMPACT,
                    cx,
                ),
                round_play_button(
                    "bilibili-continue",
                    PlayerIcon::Play,
                    if current.is_some() {
                        t!("Continue listening")
                    } else {
                        t!("Play")
                    },
                    32.0,
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
                cx,
            )
        } else {
            compact_continue_card(
                t!("Start with a favorite folder.").into(),
                "".into(),
                self.render_bilibili_thumbnail(None, tokens::HERO_COVER_COMPACT, cx),
                Button::new("bilibili-browse-folders")
                    .icon(AppIcon::ChevronRight)
                    .tooltip(t!("Browse favorite folders"))
                    .accessibility_label(t!("Browse favorite folders"))
                    .ghost()
                    .small()
                    .w(px(32.0))
                    .h(px(32.0))
                    .disabled(self.api.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate_to(View::Playlists);
                        cx.notify();
                    })),
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
        v_flex()
            .gap_6()
            .child(self.page_header(t!("Home"), t!("Your Bilibili listening, in one place."), cx))
            .when(self.api.is_none(), |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(palette(cx).text_secondary)
                        .child(self.state.status.clone()),
                )
            })
            .child(bilibili_home_lead_row(
                self.shell_layout(window).content_width,
                continue_card,
                self.render_bilibili_folder_shortcuts(&folders, has_visited, cx),
            ))
            .when(self.state.bilibili.history_loading(), |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(palette(cx).text_secondary)
                        .child(t!("Loading listening history…")),
                )
            })
            .when(self.state.bilibili.history_error().is_some(), |this| {
                this.child(
                    Button::new("retry-bilibili-history")
                        .label(t!("Retry listening history"))
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_bilibili_home();
                            cx.notify();
                        })),
                )
            })
            .child(section_title(t!("Recently played"), cx))
            .when(
                recent.is_empty()
                    && !self.state.bilibili.history_loading()
                    && self.state.bilibili.history_error().is_none()
                    && self.api.is_some(),
                |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(palette(cx).text_secondary)
                            .child(t!("No listening history yet")),
                    )
                },
            )
            .when(!recent.is_empty(), |this| {
                this.child(cover_grid(
                    columns,
                    recent
                        .iter()
                        .map(|song| {
                            let target = (*song).clone();
                            v_flex()
                                .min_w_0()
                                .gap_2()
                                .id(SharedString::from(format!(
                                    "bilibili-recent-card-{}",
                                    song.id
                                )))
                                .child(recent_play_cover(
                                    format!("bilibili-recent-cover-{}", song.id).into(),
                                    columns.width,
                                    self.render_bilibili_thumbnail(
                                        self.song_cover_image(song).cloned(),
                                        columns.width,
                                        cx,
                                    ),
                                    round_play_button(
                                        SharedString::from(format!("bilibili-recent-{}", song.id)),
                                        PlayerIcon::Play,
                                        t!("Play"),
                                        tokens::HOVER_PLAY_BUTTON,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if this.browsing_owns_song(&target) {
                                                this.replace_queue_and_play(
                                                    vec![target.clone()],
                                                    0,
                                                    QueueOrigin::Manual,
                                                );
                                                cx.notify();
                                            }
                                        },
                                    )),
                                ))
                                .child(div().text_sm().truncate().child(song.title.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(palette(cx).text_secondary)
                                        .truncate()
                                        .child(song.artist.clone()),
                                )
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                ))
            })
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
        let columns = self.content_columns(window);
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
        v_flex()
            .gap_6()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().min_w_0().child(self.page_header(
                        t!("Favorite folders"),
                        t!(
                            "%{count} favorite folders",
                            count = self.state.playlists.items().len()
                        ),
                        cx,
                    )))
                    .child(self.header_refresh_button(
                        "refresh-bilibili-folders",
                        t!("Refresh favorites"),
                        self.state.playlists.list_loading(),
                        cx,
                        |this, _| this.load_playlists(),
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .h(px(tokens::SEARCH_CONTROL_HEIGHT))
                    .px_4()
                    .items_center()
                    .rounded_full()
                    .bg(palette(cx).sunken)
                    .border_1()
                    .border_color(if focused {
                        cx.theme().ring
                    } else {
                        cx.theme().transparent
                    })
                    .child(
                        Input::new(&self.bilibili_folder_search)
                            .aria_label(t!("Find a favorite folder…"))
                            .appearance(false)
                            .w_full()
                            .min_w_0()
                            .px_0()
                            .prefix(
                                Icon::new(KitIcon::Search)
                                    .small()
                                    .text_color(palette(cx).text_muted),
                            ),
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
                    }),
            )
            .when_some(self.state.playlists.list_error(), |this, error| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.to_string()),
                )
            })
            .when(
                self.state.playlists.list_loading() && self.state.playlists.items().is_empty(),
                |this| this.child(self.render_album_skeleton_grid(window, cx)),
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
                    this.child(
                        div()
                            .text_sm()
                            .text_color(palette(cx).text_secondary)
                            .child(if self.state.playlists.items().is_empty() {
                                t!("Save videos to your favorites in Bilibili, then refresh here.")
                            } else {
                                t!("No matching favorite folders.")
                            }),
                    )
                },
            )
            .when(folders.len() > limit, |this| {
                this.child(
                    Button::new("more-bilibili-folders")
                        .label(t!("Show more folders"))
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.bilibili.show_more_folders();
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }
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
    cx: &App,
) -> gpui_kit::AnyElement {
    let tooltip = title.clone();
    raised_surface(
        v_flex()
            .w_full()
            .min_w_0()
            .min_h(px(tokens::HERO_CARD_COMPACT_HEIGHT))
            .p_4()
            .gap_3(),
        tokens::radius::LARGE,
        cx,
    )
    .id("bilibili-continue-card")
    .debug_selector(|| "bilibili-continue-card".into())
    .child(section_title(t!("Continue listening"), cx))
    .child(
        h_flex()
            .id("bilibili-continue-main")
            .debug_selector(|| "bilibili-continue-main".into())
            .w_full()
            .min_w_0()
            .items_center()
            .gap_3()
            .child(thumbnail)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .id("bilibili-continue-title")
                            .debug_selector(|| "bilibili-continue-title".into())
                            .text_size(px(tokens::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .child(title),
                    )
                    .when(!artist.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(palette(cx).text_secondary)
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
    action: Button,
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
        .child(
            div()
                .absolute()
                .right_2()
                .bottom_2()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(action),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
