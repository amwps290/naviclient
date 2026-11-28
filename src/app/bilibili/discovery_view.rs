//! Bilibili search cards and a bounded, virtualized video-part preview.

use crate::app::prelude::*;
use crate::app::style::cover_tint::{tint_background, TintSurface};
use crate::app::widgets::shelf::{cover_grid, shelf_section};
use crate::source::bilibili::discovery::MAX_SEARCH_PAGES;
use gpui_kit::Focusable;

impl NavidromeApp {
    pub(in crate::app) fn render_bilibili_discovery(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let state = &self.state.bilibili_discovery;
        let columns = self.video_content_columns(window);
        let focused = self
            .bilibili_search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let has_query = !self.bilibili_search_input.read(cx).value().trim().is_empty();
        let border_idle = if dark {
            hsla(0.0, 0.0, 1.0, 0.08)
        } else {
            p.border.opacity(0.80)
        };
        let mut content =
            v_flex().w_full().gap(px(tokens::SECTION_GAP)).child(
                v_flex()
                    .w_full()
                    .gap_5()
                    .child(self.page_header(
                        t!("Search"),
                        t!("Search Bilibili videos, or open a BV/AV ID or official link."),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .w_full()
                            .max_w(px(860.0))
                            .items_center()
                            .gap_2()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(tokens::SEARCH_CONTROL_HEIGHT))
                                    .pl_4()
                                    .pr_1p5()
                                    .gap_2()
                                    .items_center()
                                    .rounded(px(tokens::radius::FULL))
                                    .bg(p.sunken)
                                    .border_1()
                                    .border_color(if focused {
                                        cx.theme().ring
                                    } else {
                                        border_idle
                                    })
                                    .child(
                                        Input::new(&self.bilibili_search_input)
                                            .appearance(false)
                                            .w_full()
                                            .min_w_0()
                                            .px_0()
                                            .gap_3()
                                            .text_size(px(tokens::BODY))
                                            .aria_label(t!(
                                            "Search Bilibili videos or paste a BV/AV ID or link…"
                                        ))
                                            .prefix(
                                                Icon::new(KitIcon::Search)
                                                    .small()
                                                    .text_color(if focused {
                                                        p.brand
                                                    } else {
                                                        p.text_muted
                                                    }),
                                            ),
                                    )
                                    .when(has_query, |this| {
                                        this.child(
                                            icon_button(
                                                "clear-bilibili-search-input",
                                                AppIcon::Close,
                                                t!("Clear"),
                                                tokens::ICON_BUTTON_XS,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.bilibili_search_input.update(
                                                    cx,
                                                    |input, cx| input.set_value("", window, cx),
                                                );
                                                this.state.bilibili_discovery.input_error = None;
                                                cx.notify();
                                            })),
                                        )
                                    })
                                    .child(
                                        primary_button("bilibili-submit-search")
                                            .label(t!("Search"))
                                            .px_5()
                                            .rounded(px(tokens::radius::FULL))
                                            .h(px(tokens::SEARCH_CONTROL_HEIGHT - 8.0))
                                            .loading(state.search_loading())
                                            .disabled(self.api.is_none())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.submit_bilibili_discovery(cx)
                                            })),
                                    ),
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
                    .when_some(state.input_error.as_deref(), |this, error| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .child(error.to_string()),
                        )
                    })
                    .when_some(state.search_error(), |this, error| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .text_color(cx.theme().danger)
                                        .child(error.to_string()),
                                )
                                .child(
                                    surface_button("retry-bilibili-search")
                                        .label(t!("Retry"))
                                        .compact_action()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.search_bilibili_page(
                                                this.state.bilibili_discovery.keyword.clone(),
                                                this.state.bilibili_discovery.page,
                                                cx,
                                            );
                                        })),
                                ),
                        )
                    }),
            );
        if state.preview_visible() {
            content = content.child(self.render_bilibili_video_preview(cx));
        }
        if state.search_loading() && state.results.is_none() {
            content = content.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .text_color(p.text_secondary)
                            .child(t!("Searching Bilibili…")),
                    )
                    .child(self.render_video_skeleton_grid(window, cx)),
            );
        }
        if let Some(page) = &state.results {
            let source = page.source.clone();
            let cards = page
                .videos
                .iter()
                .map(|video| {
                    let bvid_card = video.bvid.clone();
                    let bvid_btn = video.bvid.clone();
                    let owner_card = source.clone();
                    let owner_btn = source.clone();
                    let favorite_owner = source.clone();
                    let favorite_bvid = video.bvid.clone();
                    let hover_key = format!("bilibili-search-{}", video.bvid);
                    let hovered = self.state.hovered_item.as_deref() == Some(hover_key.as_str());
                    let hover_key_for_cb = hover_key.clone();
                    let title_tooltip = video.title.clone();
                    let duration_label = format_duration(i32::try_from(video.duration).ok());

                    v_flex()
                        .id(SharedString::from(hover_key))
                        .min_w_0()
                        .gap_2()
                        .cursor_pointer()
                        .on_hover(cx.listener(move |this, hovering, _, cx| {
                            this.set_hovered(&hover_key_for_cb, *hovering, cx);
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.api.as_ref().map(|api| api.id()) != Some(&owner_card)
                                || this.state.bilibili_discovery.search_loading()
                            {
                                return;
                            }
                            this.open_bilibili_discovery_video(bvid_card.clone(), cx);
                        }))
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "bilibili-search-cover-{}",
                                    video.bvid
                                )))
                                .relative()
                                .w(px(columns.width))
                                .h(px(columns.width * 9.0 / 16.0))
                                .flex_none()
                                .child(self.render_bilibili_thumbnail_styled(
                                    video
                                        .cover
                                        .as_deref()
                                        .and_then(|id| self.browsing_cover_image(id))
                                        .cloned(),
                                    columns.width,
                                    tokens::radius::COVER,
                                    hovered,
                                    cx,
                                ))
                                .when(hovered, |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_2()
                                            .right_2()
                                            .rounded_full()
                                            .bg(p.raised.opacity(0.84))
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .child(
                                                icon_button(
                                                    SharedString::from(format!(
                                                        "save-bilibili-video-{}",
                                                        video.bvid
                                                    )),
                                                    AppIcon::Heart,
                                                    t!("Save video to…"),
                                                    tokens::ICON_BUTTON_XS,
                                                )
                                                .disabled(
                                                    state.search_loading()
                                                        || self.favorite_ui.writing,
                                                )
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.open_bilibili_favorite(
                                                        favorite_owner.clone(),
                                                        favorite_bvid.clone(),
                                                        super::favorite_state::FavoriteDialogKind::Picker,
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                            ),
                                    )
                                })
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
                                                primary_button(SharedString::from(format!(
                                                    "open-bilibili-video-{}",
                                                    video.bvid
                                                )))
                                                .icon(AppIcon::ChevronRight)
                                                .tooltip(t!("Open video parts"))
                                                .accessibility_label(t!("Open video parts"))
                                                .rounded(px(tokens::radius::FULL))
                                                .w(px(tokens::PLAY_BUTTON_COMPACT))
                                                .h(px(tokens::PLAY_BUTTON_COMPACT))
                                                .disabled(state.search_loading())
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    if this.api.as_ref().map(|api| api.id())
                                                        != Some(&owner_btn)
                                                        || this
                                                            .state
                                                            .bilibili_discovery
                                                            .search_loading()
                                                    {
                                                        return;
                                                    }
                                                    this.open_bilibili_discovery_video(
                                                        bvid_btn.clone(),
                                                        cx,
                                                    );
                                                })),
                                            ),
                                    )
                                })
                                .when(!hovered, |this| {
                                    this.child(super::view::video_corner_badge(
                                        duration_label.clone().into(),
                                    ))
                                }),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "bilibili-search-title-{}",
                                            video.bvid
                                        )))
                                        .min_w_0()
                                        .text_size(px(tokens::BODY))
                                        .font_weight(FontWeight::MEDIUM)
                                        .truncate()
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(title_tooltip.clone()).build(window, cx)
                                        })
                                        .child(video.title.clone()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_size(px(tokens::META))
                                        .text_color(p.text_secondary)
                                        .truncate()
                                        .child(video.uploader.clone()),
                                ),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            let previous = page.page.saturating_sub(1);
            let next = page.page.saturating_add(1);
            let keyword_for_previous = page.keyword.clone();
            let keyword_for_next = page.keyword.clone();
            let owner_for_previous = source.clone();
            let owner_for_next = source.clone();
            let results_body = if page.videos.is_empty() && !state.search_loading() {
                super::view::bilibili_empty_state(
                    AppIcon::Video,
                    t!("No matching Bilibili videos.").into(),
                    "".into(),
                    cx,
                )
            } else {
                v_flex()
                    .gap_6()
                    .child(cover_grid(columns, cards))
                    .child(
                        v_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_center()
                                    .gap_3()
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .bg(p.sunken)
                                    .child(
                                        icon_button(
                                            "previous-bilibili-search-page",
                                            KitIcon::ChevronLeft,
                                            t!("Previous"),
                                            tokens::ICON_BUTTON_XS,
                                        )
                                        .disabled(page.page <= 1 || state.search_loading())
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if this.api.as_ref().map(|api| api.id())
                                                    == Some(&owner_for_previous)
                                                {
                                                    this.search_bilibili_page(
                                                        keyword_for_previous.clone(),
                                                        previous,
                                                        cx,
                                                    );
                                                }
                                            }),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(tokens::META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.text_secondary)
                                            .child(t!(
                                                "Page %{page} / %{pages}",
                                                page = page.page,
                                                pages = page.pages
                                            )),
                                    )
                                    .child(
                                        icon_button(
                                            "next-bilibili-search-page",
                                            AppIcon::ChevronRight,
                                            t!("Next"),
                                            tokens::ICON_BUTTON_XS,
                                        )
                                        .disabled(
                                            next > page.pages
                                                || next > MAX_SEARCH_PAGES
                                                || state.search_loading(),
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if this.api.as_ref().map(|api| api.id())
                                                    == Some(&owner_for_next)
                                                {
                                                    this.search_bilibili_page(
                                                        keyword_for_next.clone(),
                                                        next,
                                                        cx,
                                                    );
                                                }
                                            }),
                                        ),
                                    ),
                            )
                            .when(page.pages > MAX_SEARCH_PAGES, |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(p.text_muted)
                                        .child(t!("Search is limited to 100 pages.")),
                                )
                            }),
                    )
                    .into_any_element()
            };
            content = content.child(shelf_section(
                section_title(
                    t!(
                        "%{count} videos · %{keyword}",
                        count = page.total,
                        keyword = page.keyword
                    ),
                    cx,
                )
                .into_any_element(),
                results_body,
            ));
        } else if !state.search_loading()
            && !state.preview_visible()
            && state.search_error().is_none()
            && state.input_error.is_none()
        {
            content = content.child(super::view::bilibili_empty_state(
                AppIcon::Video,
                t!("Enter keywords, a BV/AV ID, or a video link.").into(),
                "".into(),
                cx,
            ));
        }
        content.into_any_element()
    }

    fn render_bilibili_video_preview(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let state = &self.state.bilibili_discovery;
        let cover_id = state
            .video
            .as_ref()
            .and_then(|video| video.cover.as_deref());
        let tint = self.tint_for_cover(TintSurface::HomeFeature, cover_id, cx);
        let mut panel = raised_surface(v_flex().w_full(), tokens::radius::LARGE, cx);
        let has_video = state.video.is_some() && state.video_request.is_some();
        let mut header = v_flex()
            .w_full()
            .p_5()
            .gap_4()
            .when(has_video, |this| this.rounded_t(px(tokens::radius::LARGE)))
            .when(!has_video, |this| this.rounded(px(tokens::radius::LARGE)))
            .bg(tint_background(tint, p.raised, 125.0))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_secondary)
                            .child(t!("Video parts")),
                    )
                    .child(
                        icon_button(
                            "close-bilibili-video-preview",
                            AppIcon::Close,
                            t!("Back to search results"),
                            tokens::ICON_BUTTON_SM,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.state.bilibili_discovery.close_video();
                            this.bilibili_search_input
                                .update(cx, |input, cx| input.focus(window, cx));
                            cx.notify();
                        })),
                    ),
            )
            .when(state.video_loading(), |this| {
                this.child(
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .text_color(p.text_secondary)
                                .child(t!("Loading video parts…")),
                        )
                        .child(self.render_list_skeleton(3, cx)),
                )
            })
            .when_some(state.video_error(), |this, error| {
                this.child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .child(error.to_string()),
                        )
                        .child(
                            surface_button("retry-bilibili-video-preview")
                                .label(t!("Retry"))
                                .compact_action()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_bilibili_discovery_video(
                                        this.state.bilibili_discovery.video_input.clone(),
                                        cx,
                                    )
                                })),
                        ),
                )
            });

        if let (Some(video), Some(request)) = (&state.video, state.video_request) {
            let selected = video.selected;
            let favorite_owner = video.source.clone();
            let favorite_bvid = video
                .songs
                .first()
                .and_then(|song| song.bilibili_part.as_ref())
                .map(|part| part.bvid.clone())
                .unwrap_or_default();
            header = header.child(
                h_flex()
                    .w_full()
                    .gap_5()
                    .items_center()
                    .child(self.render_bilibili_thumbnail_styled(
                        video
                            .cover
                            .as_deref()
                            .and_then(|id| self.browsing_cover_image(id))
                            .cloned(),
                        tokens::VIDEO_HERO_COVER,
                        tokens::radius::COVER,
                        false,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1p5()
                            .child(
                                serif_heading(
                                    video.title.clone(),
                                    tokens::CARD_TITLE_COMPACT,
                                    cx,
                                )
                                .truncate(),
                            )
                            .child(
                                div()
                                    .text_size(px(tokens::META))
                                    .text_color(p.text_secondary)
                                    .truncate()
                                    .child(video.uploader.clone()),
                            )
                            .child(
                                h_flex()
                                    .pt_2()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        round_play_button(
                                            "play-selected-bilibili-part",
                                            PlayerIcon::Play,
                                            t!("Play from selected part"),
                                            tokens::PLAY_BUTTON,
                                            cx,
                                        )
                                        .on_click(cx.listener(
                                            move |this, _, _, cx| {
                                                let index = this
                                                    .state
                                                    .bilibili_discovery
                                                    .video
                                                    .as_ref()
                                                    .map_or(selected, |video| video.selected);
                                                this.play_bilibili_preview(request, index, false);
                                                cx.notify();
                                            },
                                        )),
                                    )
                                    .child(
                                        capsule_button_sm("play-all-bilibili-parts")
                                            .label(t!("Play all"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.play_bilibili_preview(request, 0, false);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        capsule_button_sm("append-all-bilibili-parts")
                                            .icon(AppIcon::Queue)
                                            .label(t!("Append all parts"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.play_bilibili_preview(request, 0, true);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        capsule_button_sm("save-bilibili-preview-video")
                                            .icon(AppIcon::Heart)
                                            .label(t!("Save video to…"))
                                            .disabled_tonal(
                                                favorite_bvid.is_empty()
                                                    || self.favorite_ui.writing,
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                if this.state.bilibili_discovery.video_request
                                                    == Some(request)
                                                {
                                                    this.open_bilibili_favorite(
                                                        favorite_owner.clone(),
                                                        favorite_bvid.clone(),
                                                        super::favorite_state::FavoriteDialogKind::Picker,
                                                        window,
                                                        cx,
                                                    );
                                                }
                                            })),
                                    ),
                            ),
                    ),
            );
            panel = panel.child(header).child(
                div()
                    .w_full()
                    .border_t_1()
                    .border_color(p.border.opacity(0.7))
                    .p_3()
                    .child(
                        uniform_list(
                            "bilibili-video-parts",
                            video.songs.len(),
                            cx.processor(move |this, range: Range<usize>, _, cx| {
                                if this.state.bilibili_discovery.video_request != Some(request) {
                                    return range.map(|_| div().into_any_element()).collect();
                                }
                                let Some(video) = this.state.bilibili_discovery.video.as_ref()
                                else {
                                    return range.map(|_| div().into_any_element()).collect();
                                };
                                let p = palette(cx);
                                range
                                    .map(|index| {
                                        let Some(song) = video.songs.get(index) else {
                                            return div().into_any_element();
                                        };
                                        let row_key = format!("bilibili-part-{request:?}-{index}");
                                        let selected_row = video.selected == index;
                                        let hovered_row = this.state.hovered_item.as_deref()
                                            == Some(row_key.as_str());
                                        let hover_key_for_cb = row_key.clone();
                                        h_flex()
                                            .id(SharedString::from(row_key))
                                            .w_full()
                                            .h(px(44.0))
                                            .items_center()
                                            .gap_3()
                                            .px_3()
                                            .rounded(px(tokens::radius::ROW))
                                            .when(selected_row, |row| row.bg(p.brand_soft))
                                            .when(!selected_row, |row| {
                                                row.hover(|style| style.bg(p.sunken))
                                            })
                                            .cursor_pointer()
                                            .on_hover(cx.listener(move |this, hovering, _, cx| {
                                                this.set_hovered(&hover_key_for_cb, *hovering, cx);
                                            }))
                                            .child(
                                                div()
                                                    .w(px(40.0))
                                                    .flex_none()
                                                    .text_size(px(tokens::CAPTION))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(if selected_row {
                                                        p.brand
                                                    } else {
                                                        p.text_muted
                                                    })
                                                    .child(format!("P{}", index + 1)),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(tokens::BODY))
                                                    .when(selected_row, |d| {
                                                        d.font_weight(FontWeight::MEDIUM)
                                                            .text_color(p.brand)
                                                    })
                                                    .truncate()
                                                    .child(song.title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .text_size(px(tokens::CAPTION))
                                                    .text_color(p.text_secondary)
                                                    .child(format_duration(song.duration)),
                                            )
                                            .child(
                                                div()
                                                    .w(px(32.0))
                                                    .h(px(32.0))
                                                    .flex_none()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .when(selected_row || hovered_row, |slot| {
                                                        slot.child(
                                                            round_play_button(
                                                                SharedString::from(format!(
                                                                    "bilibili-play-part-{request:?}-{index}"
                                                                )),
                                                                PlayerIcon::Play,
                                                                t!("Play"),
                                                                30.0,
                                                                cx,
                                                            )
                                                            .on_mouse_down(
                                                                MouseButton::Left,
                                                                |_, _, cx| cx.stop_propagation(),
                                                            )
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    cx.stop_propagation();
                                                                    this.play_bilibili_preview(
                                                                        request, index, false,
                                                                    );
                                                                    cx.notify();
                                                                },
                                                            )),
                                                        )
                                                    }),
                                            )
                                            .on_click(cx.listener(
                                                move |this,
                                                      event: &gpui_kit::ClickEvent,
                                                      _,
                                                      cx| {
                                                    if !this
                                                        .select_bilibili_preview_part(request, index)
                                                    {
                                                        return;
                                                    }
                                                    if matches!(
                                                        crate::app::shell::view_helpers::song_row_click_action(
                                                            event.modifiers(),
                                                            event.click_count(),
                                                        ),
                                                        crate::app::shell::view_helpers::SongRowClickAction::Play
                                                    ) {
                                                        this.play_bilibili_preview(
                                                            request, index, false,
                                                        );
                                                    }
                                                    cx.notify();
                                                },
                                            ))
                                            .into_any_element()
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.bilibili_parts_scroll)
                        .h(px((video.songs.len() as f32 * 44.0).min(308.0))),
                    ),
            );
        } else {
            panel = panel.child(header);
        }
        panel.into_any_element()
    }
}
