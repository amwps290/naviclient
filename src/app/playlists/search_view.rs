//! An anchored horizontal search reveal and feedback in the existing status row.

use super::library::PlaylistLibrary;
use crate::app::prelude::*;
use gpui_kit::component::input::Escape;
use gpui_kit::{Focusable, SpringAnimation, SpringConfig};

const SEARCH_HEIGHT: f32 = 32.0;
const SEARCH_MIN_WIDTH: f32 = 216.0;
const SEARCH_MAX_WIDTH: f32 = 384.0;
const SEARCH_REVEAL_SPRING: SpringConfig = SpringConfig::new(900.0, 60.0, 1.0);
const SEARCH_BUTTON_SIZE: f32 = SEARCH_HEIGHT - 8.0;
const SEARCH_CONTENT_HEIGHT: f32 = SEARCH_HEIGHT - 2.0;

impl NavidromeApp {
    pub(in crate::app) fn render_playlist_search(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        render_playlist_search_controls(
            &self.state.playlists,
            &self.playlist_search_input,
            self.playlist_search_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
            cx.listener(|this, _, window, cx| this.toggle_playlist_search(window, cx)),
            cx.listener(|this, _: &Escape, window, cx| {
                if this
                    .playlist_search_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
                {
                    if this.playlist_search_input.read(cx).value().is_empty() {
                        this.close_playlist_search(window, cx);
                    } else {
                        this.playlist_search_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.set_playlist_search_query(String::new(), cx);
                    }
                } else {
                    cx.propagate();
                }
            }),
            cx.listener(|this, _, window, cx| this.close_playlist_search(window, cx)),
            cx,
        )
    }

    pub(in crate::app) fn render_playlist_search_feedback(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        render_playlist_search_feedback(
            &self.state.playlists,
            cx.listener(|this, _, _, cx| {
                this.state.playlists.search.retry();
                cx.notify();
            }),
            cx,
        )
    }
}

fn playlist_search_button(
    expanded: bool,
    on_toggle: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Button {
    let label = if expanded {
        t!("Close playlist search")
    } else {
        t!("Search within this playlist")
    };
    Button::new("toggle-playlist-search")
        .debug_selector(|| "toggle-playlist-search".into())
        .icon(KitIcon::Search)
        .accessibility_label(label.clone())
        .tooltip(label)
        .ghost()
        .text_color(palette(cx).text_muted)
        .rounded_full()
        .small()
        .size(px(SEARCH_BUTTON_SIZE))
        .on_click(on_toggle)
}

pub(in crate::app) fn playlist_search_label(playlists: &PlaylistLibrary) -> String {
    let search = &playlists.search;
    let total = playlists.songs().len();
    if !playlists.detail_ready() {
        t!("Search available when loading completes").to_string()
    } else if search.pending() {
        t!("Filtering tracks…").to_string()
    } else if search.error().is_some() {
        t!("Playlist search failed").to_string()
    } else if search.filtered() {
        t!(
            "%{matched} / %{total} tracks",
            matched = search.indices().map_or(0, <[usize]>::len),
            total = total
        )
        .to_string()
    } else {
        t!("%{count} tracks", count = total).to_string()
    }
}

pub(in crate::app) fn render_playlist_search_controls(
    playlists: &PlaylistLibrary,
    input: &Entity<InputState>,
    focused: bool,
    on_toggle: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    on_escape: impl Fn(&Escape, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui_kit::AnyElement {
    let expanded = playlists.search.expanded;
    let p = palette(cx);
    // Reserve the same toolbar lane in both states, but paint only the growing
    // capsule. The icon is embedded in its left end and never moves.
    let contents = expanded.then(|| {
        h_flex()
            .id("playlist-search-bar")
            .debug_selector(|| "playlist-search-bar".into())
            .flex_1()
            .min_w_0()
            .h(px(SEARCH_CONTENT_HEIGHT))
            .items_center()
            .child(render_playlist_search_input(input, on_escape))
            .child(
                div()
                    .w(px(SEARCH_CONTENT_HEIGHT))
                    .h(px(SEARCH_CONTENT_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Button::new("close-playlist-search")
                            .debug_selector(|| "close-playlist-search".into())
                            .icon(AppIcon::Close)
                            .accessibility_label(t!("Close playlist search"))
                            .tooltip(t!("Close playlist search"))
                            .ghost()
                            .text_color(p.text_muted)
                            .rounded_full()
                            .small()
                            .size(px(SEARCH_BUTTON_SIZE))
                            .on_click(on_close),
                    ),
            )
            .into_any_element()
    });
    let capsule = h_flex()
        .id("playlist-search-reveal")
        .debug_selector(|| "playlist-search-reveal".into())
        .flex_none()
        .min_w(px(SEARCH_HEIGHT))
        .h(px(SEARCH_HEIGHT))
        .items_center()
        .rounded_full()
        .bg(p.sunken)
        // This is the one surface/edge for icon, editor and close button.
        // The constant inset border avoids a clipped outer focus ring.
        .border_1()
        .border_color(if focused {
            cx.theme().ring
        } else {
            cx.theme().transparent
        })
        .overflow_hidden()
        .child(
            div()
                .w(px(SEARCH_CONTENT_HEIGHT))
                .h(px(SEARCH_CONTENT_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(playlist_search_button(expanded, on_toggle, cx)),
        )
        .children(contents)
        .with_spring(
            "playlist-search-width",
            SpringAnimation::new(SEARCH_REVEAL_SPRING)
                .to(if expanded { 1.0_f32 } else { 0.0_f32 })
                .from(0.0),
            move |this, progress| this.w(relative(progress.clamp(0.0, 1.0))),
        );
    h_flex()
        .id("playlist-search-anchor")
        .debug_selector(|| "playlist-search-anchor".into())
        .flex_1()
        .min_w(px(SEARCH_MIN_WIDTH))
        .max_w(px(SEARCH_MAX_WIDTH))
        .h(px(SEARCH_HEIGHT))
        .items_center()
        .child(capsule)
        .into_any_element()
}

pub(in crate::app) fn render_playlist_search_feedback(
    playlists: &PlaylistLibrary,
    on_retry: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui_kit::AnyElement {
    let label = playlist_search_label(playlists);
    let tooltip = if playlists.search.filtered() {
        t!("Clear search to reorder tracks. Play all still uses the entire playlist.").to_string()
    } else {
        label.clone()
    };
    h_flex()
        .flex_none()
        .max_w(px(SEARCH_MAX_WIDTH))
        .items_center()
        .gap_2()
        .child(
            div()
                .id("playlist-search-count")
                .debug_selector(|| "playlist-search-count".into())
                .min_w_0()
                .text_size(px(tokens::META))
                .text_color(palette(cx).text_secondary)
                .truncate()
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .child(label),
        )
        .when(
            playlists.search.error().is_some() && playlists.detail_ready(),
            |this| {
                this.child(
                    Button::new("retry-playlist-search")
                        .label(t!("Retry"))
                        .debug_selector(|| "retry-playlist-search".into())
                        .ghost()
                        .compact_action()
                        .on_click(on_retry),
                )
            },
        )
        .into_any_element()
}

pub(in crate::app) fn render_playlist_search_empty(
    playlists: &PlaylistLibrary,
    cx: &App,
) -> gpui_kit::AnyElement {
    if playlists.songs().is_empty() || !playlists.detail_ready() {
        return super::detail_view::render_playlist_placeholder(playlists, cx);
    }
    let title = if playlists.search.pending() {
        t!("Filtering tracks…")
    } else if playlists.search.error().is_some() {
        t!("Playlist search failed")
    } else {
        t!("No matching tracks in this playlist")
    };
    let description = if playlists.search.pending() {
        t!("%{count} tracks", count = playlists.songs().len())
    } else {
        t!("Try another keyword or clear the playlist search.")
    };
    crate::app::songs::view::render_empty_song_list(title.to_string(), description.to_string(), cx)
}

/// Only an input action is intercepted; raw text keys and IME stay native.
fn render_playlist_search_input(
    input: &Entity<InputState>,
    on_escape: impl Fn(&Escape, &mut Window, &mut App) + 'static,
) -> gpui_kit::AnyElement {
    div()
        .id("playlist-search-input")
        .debug_selector(|| "playlist-search-input".into())
        .flex_1()
        .min_w_0()
        .pl_1()
        .on_action(on_escape)
        .child(
            Input::new(input)
                .aria_label(t!("Search within this playlist"))
                .accessibility_id("playlist-local-search")
                .appearance(false)
                .w_full()
                .min_w_0()
                .h(px(SEARCH_CONTENT_HEIGHT))
                .px_0()
                .py_0(),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
