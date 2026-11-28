//! Folder browse state, breadcrumb trail, async loading, and directory view rendering.

use super::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::app::prelude::*;
use crate::app::widgets::shelf::shelf_section;
use crate::models::{FolderItem, FolderListing};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct FolderCrumb {
    pub(in crate::app) id: String,
    pub(in crate::app) name: String,
}

#[derive(Default)]
pub(in crate::app) struct FolderBrowser {
    crumbs: Vec<FolderCrumb>,
    listing: FolderListing,
    load: LibraryLoad,
    loaded_once: bool,
}

impl FolderBrowser {
    pub(in crate::app) fn crumbs(&self) -> &[FolderCrumb] {
        &self.crumbs
    }

    pub(in crate::app) fn current_id(&self) -> &str {
        self.crumbs.last().map(|c| c.id.as_str()).unwrap_or("")
    }

    pub(in crate::app) fn folders(&self) -> &[FolderItem] {
        &self.listing.folders
    }

    pub(in crate::app) fn songs(&self) -> &[Song] {
        &self.listing.songs
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn error(&self) -> Option<&str> {
        self.load.error()
    }

    pub(in crate::app) fn loaded_once(&self) -> bool {
        self.loaded_once
    }

    pub(in crate::app) fn open_root(&mut self) {
        self.crumbs.clear();
    }

    pub(in crate::app) fn push_folder(&mut self, id: String, name: String) {
        self.crumbs.push(FolderCrumb { id, name });
    }

    pub(in crate::app) fn jump_to_crumb(&mut self, index: usize) {
        if index < self.crumbs.len() {
            self.crumbs.truncate(index + 1);
        }
    }

    pub(in crate::app) fn begin(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        self.load.begin(requests)
    }

    pub(in crate::app) fn complete(
        &mut self,
        request: LibraryRequest,
        result: Result<FolderListing, String>,
    ) -> Option<Result<&FolderListing, &str>> {
        Some(match self.load.complete(request, result)? {
            Ok(listing) => {
                self.loaded_once = true;
                self.listing = listing;
                Ok(&self.listing)
            }
            Err(error) => Err(error),
        })
    }
}

impl NavidromeApp {
    pub(in crate::app) fn load_current_folder(&mut self) {
        let Some(api) = self.api.clone() else { return };
        if !api.capabilities().folders {
            return;
        }
        let folder_id = self.state.folders.current_id().to_string();
        let request = self.state.folders.begin(&mut self.library_requests);
        if self.offline_mode_active() {
            let listing = self.offline_store.folder_listing(api.id(), &folder_id);
            self.finish_folder_listing(request, Ok(listing));
            return;
        }
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.folder_listing(&folder_id).await.map_err(error_message);
            let _ = tx.send(Msg::FolderListing { request, result });
        });
    }

    pub(in crate::app) fn open_folder_root(&mut self) {
        self.clear_song_multi();
        self.state.folders.open_root();
        self.load_current_folder();
    }

    pub(in crate::app) fn open_subfolder(&mut self, id: String, name: String) {
        self.clear_song_multi();
        self.state.folders.push_folder(id, name);
        self.load_current_folder();
    }

    pub(in crate::app) fn jump_to_folder_crumb(&mut self, index: usize) {
        self.clear_song_multi();
        self.state.folders.jump_to_crumb(index);
        self.load_current_folder();
    }

    pub(in crate::app) fn finish_folder_listing(
        &mut self,
        request: LibraryRequest,
        result: Result<FolderListing, String>,
    ) {
        let Some(result) = self.state.folders.complete(request, result) else {
            return;
        };
        match result {
            Ok(listing) => {
                let covers: Vec<String> = listing
                    .folders
                    .iter()
                    .filter_map(|f| f.cover_art.clone())
                    .chain(listing.songs.iter().filter_map(|s| s.cover_art.clone()))
                    .take(60)
                    .collect();
                self.state.clear_song_selection_for_scope(SongScope::Folder);
                self.preload_covers(covers);
            }
            Err(error) => {
                let error = error.to_string();
                self.notify_error(error);
            }
        }
    }

    pub(in crate::app) fn render_folders_view(
        &self,
        _window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let folders = self.state.folders.folders();
        let songs = self.state.folders.songs();
        let crumbs = self.state.folders.crumbs();
        let loading = self.state.folders.loading();
        let error = self.state.folders.error();

        let subtitle = t!(
            "%{folders} folders · %{tracks} tracks",
            folders = folders.len(),
            tracks = songs.len()
        );

        // Breadcrumb bar: Root > Folder A > Folder B
        let mut breadcrumb_bar = h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .py_1p5()
            .rounded(px(tokens::radius::CONTROL))
            .bg(if dark {
                hsla(0.0, 0.0, 1.0, 0.03)
            } else {
                p.sunken.opacity(0.72)
            })
            .border_1()
            .border_color(if dark {
                hsla(0.0, 0.0, 1.0, 0.07)
            } else {
                p.border.opacity(0.75)
            })
            .child(
                ghost_button("folder-crumb-root")
                    .icon(AppIcon::Folder)
                    .label(t!("Root"))
                    .dense_action()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_folder_root();
                        cx.notify();
                    })),
            );

        for (index, crumb) in crumbs.iter().enumerate() {
            let is_last = index + 1 == crumbs.len();
            breadcrumb_bar = breadcrumb_bar
                .child(
                    Icon::new(AppIcon::ChevronRight)
                        .with_size(px(12.0))
                        .text_color(p.text_muted),
                )
                .child(if is_last {
                    div()
                        .px_2()
                        .py_0p5()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text)
                        .child(crumb.name.clone())
                        .into_any_element()
                } else {
                    ghost_button(SharedString::from(format!("folder-crumb-{index}")))
                        .label(crumb.name.clone())
                        .dense_action()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.jump_to_folder_crumb(index);
                            cx.notify();
                        }))
                        .into_any_element()
                });
        }

        let header = h_flex()
            .w_full()
            .items_end()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .child(self.page_header(t!("Folders"), subtitle, cx)),
            )
            .when(!songs.is_empty(), |this| {
                this.child(
                    capsule_button("play-folder-tracks")
                        .icon(PlayerIcon::Play)
                        .label(t!("Play all"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.play_song_at_scope(SongScope::Folder, 0);
                            cx.notify();
                        })),
                )
                .child(
                    capsule_button("append-folder-tracks")
                        .icon(AppIcon::Queue)
                        .label(t!("Append all"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.append_songs_at_scope(SongScope::Folder);
                            cx.notify();
                        })),
                )
            })
            .child(self.header_refresh_button(
                "refresh-folder",
                t!("Refresh folder"),
                loading,
                cx,
                |this, _cx| this.load_current_folder(),
            ));

        let mut content = v_flex().gap_5().child(header).child(breadcrumb_bar);

        if error.is_some() {
            content = content.child(self.render_retry_row("retry-folder", cx, |this, cx| {
                this.load_current_folder();
                cx.notify();
            }));
        }

        if loading && folders.is_empty() && songs.is_empty() {
            content = content.child(self.render_list_skeleton(6, cx));
            return content.into_any_element();
        }

        if !folders.is_empty() {
            let folder_cards = h_flex().w_full().flex_wrap().gap_2p5().children(
                folders.iter().enumerate().map(|(idx, folder)| {
                    let id = folder.id.clone();
                    let name = folder.name.clone();
                    let count_text = folder.song_count.map(|c| t!("%{count} tracks", count = c));
                    h_flex()
                        .id(SharedString::from(format!("folder-card-{idx}")))
                        .w(px(220.0))
                        .px_3()
                        .py_2p5()
                        .gap_2p5()
                        .items_center()
                        .rounded(px(tokens::radius::COVER))
                        .bg(if dark {
                            hsla(0.0, 0.0, 1.0, 0.035)
                        } else {
                            p.raised
                        })
                        .border_1()
                        .border_color(if dark {
                            hsla(0.0, 0.0, 1.0, 0.07)
                        } else {
                            p.border.opacity(0.8)
                        })
                        .cursor_pointer()
                        .hover(move |style| style.bg(p.sunken).border_color(p.brand.opacity(0.35)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_subfolder(id.clone(), name.clone());
                            cx.notify();
                        }))
                        .child(
                            div()
                                .size(px(34.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(tokens::radius::CONTROL))
                                .bg(p.brand_soft)
                                .child(
                                    Icon::new(AppIcon::Folder)
                                        .with_size(px(16.0))
                                        .text_color(p.brand),
                                ),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .truncate()
                                        .child(folder.name.clone()),
                                )
                                .when_some(count_text, |this, text| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(p.text_muted)
                                            .truncate()
                                            .child(text),
                                    )
                                }),
                        )
                }),
            );
            content = content.child(shelf_section(
                section_title(t!("Subfolders"), cx).into_any_element(),
                folder_cards.into_any_element(),
            ));
        }

        if !songs.is_empty() {
            content = content.child(shelf_section(
                section_title(t!("Songs"), cx).into_any_element(),
                self.render_song_list(SongScope::Folder, cx),
            ));
        } else if folders.is_empty() && !loading {
            content = content.child(
                v_flex()
                    .w_full()
                    .min_h(px(180.0))
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        Icon::new(AppIcon::Folder)
                            .with_size(px(30.0))
                            .text_color(p.text_muted.opacity(0.6)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.text_secondary)
                            .child(t!("This folder is empty.")),
                    ),
            );
        }

        content.into_any_element()
    }
}
