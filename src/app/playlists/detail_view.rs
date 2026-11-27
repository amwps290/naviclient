//! Playlist loading feedback and counts that distinguish previews from empty results.

use super::library::PlaylistLibrary;
use crate::app::prelude::*;
use crate::app::songs::view::render_empty_song_list;

pub(in crate::app) fn render_playlist_placeholder(
    playlists: &PlaylistLibrary,
    cx: &App,
) -> gpui_kit::AnyElement {
    if !playlists.detail_ready() && playlists.detail_error().is_none() {
        return div()
            .id("playlist-loading-skeleton")
            .debug_selector(|| "playlist-loading-skeleton".into())
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(crate::app::widgets::skeleton::render_list_skeleton(6, cx))
            .into_any_element();
    }
    let (title, description) = if playlists.detail_error().is_some() {
        (
            t!("Playlist loading failed").to_string(),
            t!("Use Retry to load this playlist again.").to_string(),
        )
    } else if playlists.unavailable_count() > 0 {
        (
            t!("No available tracks").to_string(),
            t!(
                "All %{count} favorites are unavailable.",
                count = playlists.unavailable_count()
            )
            .to_string(),
        )
    } else {
        (
            t!("No tracks here yet").to_string(),
            t!("Tracks will appear here when they are available.").to_string(),
        )
    };
    render_empty_song_list(title, description, cx)
}

pub(in crate::app) fn playlist_track_label(playlists: &PlaylistLibrary, bilibili: bool) -> String {
    let count = playlists.songs().len();
    if playlists.detail_ready() {
        t!("%{count} tracks", count = count).to_string()
    } else if count > 0 {
        t!("%{count} tracks loaded", count = count).to_string()
    } else if playlists.detail_error().is_some() {
        t!("Loading incomplete").to_string()
    } else if bilibili {
        t!("Loading favorites…").to_string()
    } else {
        t!("Loading playlist…").to_string()
    }
}

#[cfg(test)]
mod tests;

impl NavidromeApp {
    pub(in crate::app) fn render_playlist_status(
        &self,
        bilibili: bool,
        read_only: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        render_playlist_status_content(
            &self.state.playlists,
            bilibili,
            read_only,
            cx.listener(|this, _, _, cx| {
                this.load_playlist_detail();
                cx.notify();
            }),
            cx,
        )
    }
}

pub(in crate::app) fn render_playlist_status_content(
    playlists: &PlaylistLibrary,
    bilibili: bool,
    read_only: bool,
    on_retry: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui_kit::AnyElement {
    let error = playlists.detail_error();
    let label = if playlists.editing() {
        t!("Saving playlist…").to_string()
    } else if !playlists.detail_ready() && error.is_none() {
        if !playlists.songs().is_empty() {
            t!(
                "Loading… · %{count} tracks shown",
                count = playlists.songs().len()
            )
            .to_string()
        } else if bilibili {
            t!("Loading favorites…").to_string()
        } else {
            t!("Loading playlist…").to_string()
        }
    } else if let Some(error) = error {
        format!("{} · {error}", t!("Loading incomplete"))
    } else if bilibili {
        t!("All pages loaded · Video order and part order preserved").to_string()
    } else if read_only {
        t!("Shared playlist · Only the owner can edit it.").to_string()
    } else if playlists.songs().is_empty() {
        t!("This playlist is empty. Use a song’s actions menu to add tracks.").to_string()
    } else {
        t!("Use a song’s actions menu to move or remove it.").to_string()
    };
    let tooltip = label.clone();
    h_flex()
        .id("playlist-load-status")
        .debug_selector(|| "playlist-load-status".into())
        .w_full()
        .flex_none()
        .h(px(tokens::PLAYLIST_STATUS_HEIGHT))
        .items_center()
        .gap_2()
        .child(
            div()
                .id("playlist-load-status-text")
                .flex_1()
                .min_w_0()
                .text_size(px(tokens::META))
                .text_color(palette(cx).text_muted)
                .truncate()
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .child(label),
        )
        .when(error.is_some(), |this| {
            this.child(
                ghost_button("retry-playlist")
                    .debug_selector(|| "retry-playlist".into())
                    .label(t!("Retry"))
                    .compact_action()
                    .disabled(playlists.editing() || playlists.detail_loading())
                    .on_click(on_retry),
            )
        })
        .into_any_element()
}
