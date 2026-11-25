//! Bilibili search cards and a bounded, virtualized video-part preview.

use crate::app::prelude::*;
use crate::app::widgets::shelf::cover_grid;
use crate::source::bilibili::discovery::MAX_SEARCH_PAGES;
use gpui_kit::Focusable;

impl NavidromeApp {
    pub(in crate::app) fn render_bilibili_discovery(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let state = &self.state.bilibili_discovery;
        let columns = self.content_columns(window);
        let focused = self
            .bilibili_search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let mut content = v_flex()
            .w_full()
            .gap_6()
            .child(self.page_header(
                t!("Search"),
                t!("Search Bilibili videos, or open a BV/AV ID or official link."),
                cx,
            ))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
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
                                Input::new(&self.bilibili_search_input)
                                    .appearance(false)
                                    .w_full()
                                    .min_w_0()
                                    .px_0()
                                    .aria_label(t!(
                                        "Search Bilibili videos or paste a BV/AV ID or link…"
                                    ))
                                    .prefix(
                                        Icon::new(KitIcon::Search)
                                            .small()
                                            .text_color(palette(cx).text_muted),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("bilibili-submit-search")
                            .label(t!("Search"))
                            .primary()
                            .rounded_full()
                            .h(px(tokens::SEARCH_CONTROL_HEIGHT))
                            .disabled(self.api.is_none())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.submit_bilibili_discovery(cx)),
                            ),
                    ),
            )
            .when(self.api.is_none(), |this| {
                this.child(div().text_sm().child(self.state.status.clone()))
            })
            .when_some(state.input_error.as_deref(), |this, error| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.to_string()),
                )
            })
            .when(state.search_loading(), |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(palette(cx).text_secondary)
                        .child(t!("Searching Bilibili…")),
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
                            Button::new("retry-bilibili-search")
                                .label(t!("Retry"))
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.search_bilibili_page(
                                        this.state.bilibili_discovery.keyword.clone(),
                                        this.state.bilibili_discovery.page,
                                        cx,
                                    );
                                })),
                        ),
                )
            });
        if state.preview_visible() {
            content = content.child(self.render_bilibili_video_preview(cx));
        }
        if let Some(page) = &state.results {
            let source = page.source.clone();
            let cards = page
                .videos
                .iter()
                .map(|video| {
                    let bvid = video.bvid.clone();
                    let owner = source.clone();
                    let favorite_owner = source.clone();
                    let favorite_bvid = video.bvid.clone();
                    let open = cx.listener(move |this, _, _, cx| {
                        if this.api.as_ref().map(|api| api.id()) != Some(&owner)
                            || this.state.bilibili_discovery.search_loading()
                        {
                            return;
                        }
                        this.open_bilibili_discovery_video(bvid.clone(), cx);
                    });
                    v_flex()
                        .id(SharedString::from(format!(
                            "bilibili-search-{}",
                            video.bvid
                        )))
                        .min_w_0()
                        .gap_2()
                        .child(super::view::recent_play_cover(
                            format!("bilibili-search-cover-{}", video.bvid).into(),
                            columns.width,
                            self.render_bilibili_thumbnail(
                                video
                                    .cover
                                    .as_deref()
                                    .and_then(|id| self.browsing_cover_image(id))
                                    .cloned(),
                                columns.width,
                                cx,
                            ),
                            Button::new(SharedString::from(format!(
                                "open-bilibili-video-{}",
                                video.bvid
                            )))
                            .icon(AppIcon::ChevronRight)
                            .tooltip(t!("Open video parts"))
                            .accessibility_label(t!("Open video parts"))
                            .primary()
                            .rounded_full()
                            .w(px(32.0))
                            .h(px(32.0))
                            .disabled(state.search_loading())
                            .on_click(open),
                        ))
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .truncate()
                                        .child(video.title.clone()),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "save-bilibili-video-{}",
                                        video.bvid
                                    )))
                                    .icon(AppIcon::Heart)
                                    .tooltip(t!("Save video to…"))
                                    .accessibility_label(t!("Save video to…"))
                                    .ghost()
                                    .small()
                                    .disabled(state.search_loading() || self.favorite_ui.writing)
                                    .on_click(cx.listener(move |this, _, window, cx| {
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
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(palette(cx).text_secondary)
                                        .truncate()
                                        .child(video.uploader.clone()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(palette(cx).text_secondary)
                                        .child(format_duration(i32::try_from(video.duration).ok())),
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
            content = content
                .child(section_title(
                    t!(
                        "%{count} videos · %{keyword}",
                        count = page.total,
                        keyword = page.keyword
                    ),
                    cx,
                ))
                .when(page.videos.is_empty() && !state.search_loading(), |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(palette(cx).text_secondary)
                            .child(t!("No matching Bilibili videos.")),
                    )
                })
                .when(!page.videos.is_empty(), |this| {
                    this.child(cover_grid(columns, cards))
                })
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(
                            Button::new("previous-bilibili-search-page")
                                .icon(KitIcon::ChevronLeft)
                                .tooltip(t!("Previous"))
                                .ghost()
                                .small()
                                .disabled(page.page <= 1 || state.search_loading())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.api.as_ref().map(|api| api.id())
                                        == Some(&owner_for_previous)
                                    {
                                        this.search_bilibili_page(
                                            keyword_for_previous.clone(),
                                            previous,
                                            cx,
                                        );
                                    }
                                })),
                        )
                        .child(div().text_sm().child(t!(
                            "Page %{page} / %{pages}",
                            page = page.page,
                            pages = page.pages
                        )))
                        .child(
                            Button::new("next-bilibili-search-page")
                                .icon(AppIcon::ChevronRight)
                                .tooltip(t!("Next"))
                                .ghost()
                                .small()
                                .disabled(
                                    next > page.pages
                                        || next > MAX_SEARCH_PAGES
                                        || state.search_loading(),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.api.as_ref().map(|api| api.id())
                                        == Some(&owner_for_next)
                                    {
                                        this.search_bilibili_page(
                                            keyword_for_next.clone(),
                                            next,
                                            cx,
                                        );
                                    }
                                })),
                        ),
                )
                .when(page.pages > MAX_SEARCH_PAGES, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(palette(cx).text_secondary)
                            .child(t!("Search is limited to 100 pages.")),
                    )
                });
        } else if !state.search_loading()
            && !state.preview_visible()
            && state.search_error().is_none()
            && state.input_error.is_none()
        {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(palette(cx).text_secondary)
                    .child(t!("Enter keywords, a BV/AV ID, or a video link.")),
            );
        }
        content.into_any_element()
    }

    fn render_bilibili_video_preview(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let state = &self.state.bilibili_discovery;
        let mut panel = raised_surface(v_flex().w_full().p_4().gap_3(), tokens::radius::LARGE, cx)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(section_title(t!("Video parts"), cx))
                    .child(
                        Button::new("close-bilibili-video-preview")
                            .icon(AppIcon::Close)
                            .tooltip(t!("Back to search results"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.state.bilibili_discovery.close_video();
                                this.bilibili_search_input
                                    .update(cx, |input, cx| input.focus(window, cx));
                                cx.notify();
                            })),
                    ),
            )
            .when(state.video_loading(), |this| {
                this.child(div().text_sm().child(t!("Loading video parts…")))
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
                            Button::new("retry-bilibili-video-preview")
                                .label(t!("Retry"))
                                .ghost()
                                .small()
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
            panel = panel.child(h_flex().w_full().gap_3().items_center()
                .child(self.render_bilibili_thumbnail(video.cover.as_deref().and_then(|id| self.browsing_cover_image(id)).cloned(), tokens::HERO_COVER_COMPACT, cx))
                .child(v_flex().flex_1().min_w_0().gap_1()
                    .child(div().text_size(px(tokens::SECTION_TITLE)).font_weight(FontWeight::SEMIBOLD).truncate().child(video.title.clone()))
                    .child(div().text_sm().text_color(palette(cx).text_secondary).truncate().child(video.uploader.clone()))))
                .child(h_flex().w_full().items_center().gap_2().flex_wrap()
                    .child(Button::new("play-selected-bilibili-part").label(t!("Play from selected part")).primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let index = this.state.bilibili_discovery.video.as_ref().map_or(selected, |video| video.selected);
                            this.play_bilibili_preview(request, index, false); cx.notify();
                        })))
                    .child(Button::new("play-all-bilibili-parts").label(t!("Play all")).ghost()
                        .on_click(cx.listener(move |this, _, _, cx| { this.play_bilibili_preview(request, 0, false); cx.notify(); })))
                    .child(Button::new("append-all-bilibili-parts").label(t!("Append all parts")).ghost()
                        .on_click(cx.listener(move |this, _, _, cx| { this.play_bilibili_preview(request, 0, true); cx.notify(); })))
                    .child(Button::new("save-bilibili-preview-video").label(t!("Save video to…")).icon(AppIcon::Heart).ghost()
                        .disabled(favorite_bvid.is_empty() || self.favorite_ui.writing)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.state.bilibili_discovery.video_request == Some(request) {
                                this.open_bilibili_favorite(favorite_owner.clone(), favorite_bvid.clone(),
                                    super::favorite_state::FavoriteDialogKind::Picker, window, cx);
                            }
                        }))))
                .child(uniform_list("bilibili-video-parts", video.songs.len(), cx.processor(move |this, range: Range<usize>, _, cx| {
                    if this.state.bilibili_discovery.video_request != Some(request) {
                        return range.map(|_| div().into_any_element()).collect();
                    }
                    let Some(video) = this.state.bilibili_discovery.video.as_ref() else { return range.map(|_| div().into_any_element()).collect() };
                    range.map(|index| {
                        let Some(song) = video.songs.get(index) else { return div().into_any_element() };
                        h_flex().id(SharedString::from(format!("bilibili-part-{request:?}-{index}")))
                            .w_full().h(px(48.0)).items_center().gap_3().px_2().rounded(px(tokens::radius::CONTROL))
                            .when(video.selected == index, |row| row.bg(palette(cx).brand_soft))
                            .cursor_pointer()
                            .child(div().w(px(48.0)).text_sm().child(format!("P{}", index + 1)))
                            .child(div().flex_1().min_w_0().text_sm().truncate().child(song.title.clone()))
                            .child(div().text_xs().text_color(palette(cx).text_secondary).child(format_duration(song.duration)))
                            .child(round_play_button(SharedString::from(format!("bilibili-play-part-{request:?}-{index}")), PlayerIcon::Play, t!("Play"), 32.0)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.play_bilibili_preview(request, index, false); cx.notify(); })))
                            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                                if !this.select_bilibili_preview_part(request, index) { return; }
                                if matches!(crate::app::shell::view_helpers::song_row_click_action(event.modifiers(), event.click_count()), crate::app::shell::view_helpers::SongRowClickAction::Play) {
                                    this.play_bilibili_preview(request, index, false);
                                }
                                cx.notify();
                            })).into_any_element()
                    }).collect::<Vec<_>>()
                })).track_scroll(&self.bilibili_parts_scroll).h(px((video.songs.len() as f32 * 48.0).min(320.0))));
        }
        panel.into_any_element()
    }
}
