use super::*;
use crate::app::shell::view_helpers::SONG_ROW_HEIGHT;
use crate::playlist::loading::{
    PlaylistFailureKind, PlaylistLoadError, PlaylistProgress, PlaylistUpdate,
};
use gpui_kit::component::Root;
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};

fn folder() -> Playlist {
    Playlist {
        id: "5".into(),
        name: "Large favorites".into(),
        song_count: Some(2000),
        ..Default::default()
    }
}

struct PreviewProbe {
    playlists: PlaylistLibrary,
    requests: LibraryRequests,
    request: LibraryRequest,
    scroll: UniformListScrollHandle,
    rendered_rows: usize,
}

impl PreviewProbe {
    fn new() -> Self {
        let mut playlists = PlaylistLibrary::default();
        let mut requests = LibraryRequests::default();
        playlists.select(folder());
        let request = playlists.begin_detail(&mut requests).unwrap();
        playlists.apply_detail_update(request, PlaylistUpdate::Metadata(folder()));
        Self {
            playlists,
            requests,
            request,
            scroll: UniformListScrollHandle::new(),
            rendered_rows: 0,
        }
    }

    fn publish(&mut self, start: usize, end: usize) {
        assert!(self
            .playlists
            .apply_detail_update(
                self.request,
                PlaylistUpdate::Batch {
                    songs: (start..end)
                        .map(|index| Song {
                            id: index.to_string(),
                            title: format!("Track {index}"),
                            ..Default::default()
                        })
                        .collect(),
                    progress: PlaylistProgress {
                        songs: end,
                        ..Default::default()
                    },
                }
            )
            .is_some());
    }

    fn fail(&mut self) {
        self.playlists.apply_detail_update(
            self.request,
            PlaylistUpdate::Failed(PlaylistLoadError {
                kind: PlaylistFailureKind::Network,
                message: "offline".into(),
            }),
        );
    }
}

impl Render for PreviewProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.playlists.songs().is_empty() {
            render_playlist_placeholder(&self.playlists, cx)
        } else {
            uniform_list(
                "playlist-preview-list",
                self.playlists.songs().len(),
                cx.processor(|probe, range: Range<usize>, _, _| {
                    probe.rendered_rows = range.len();
                    range
                        .map(|index| {
                            div()
                                .id(SharedString::from(format!("preview-row-{index}")))
                                .debug_selector(move || format!("preview-row-{index}"))
                                .w_full()
                                .h(px(SONG_ROW_HEIGHT))
                                .child(probe.playlists.songs()[index].title.clone())
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        v_flex()
            .w(px(320.0))
            .h(px(400.0))
            .min_h_0()
            .child(
                div()
                    .id("preview-header")
                    .h(px(32.0))
                    .flex_none()
                    .child(playlist_track_label(&self.playlists, true)),
            )
            .child(render_playlist_status_content(
                &self.playlists,
                true,
                true,
                cx.listener(|probe, _, _, cx| {
                    probe.request = probe.playlists.begin_detail(&mut probe.requests).unwrap();
                    probe
                        .playlists
                        .apply_detail_update(probe.request, PlaylistUpdate::Metadata(folder()));
                    cx.notify();
                }),
                cx,
            ))
            .child(body)
    }
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn on_screen(cx: &mut TestAppContext) -> (Entity<PreviewProbe>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|_| PreviewProbe::new());
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw(&mut visual);
    (probe.unwrap(), visual)
}

#[gpui_pre::test]
fn loading_is_not_an_empty_playlist_and_retry_can_complete_a_truly_empty_one(
    cx: &mut TestAppContext,
) {
    let (probe, mut visual) = on_screen(cx);
    assert!(visual.debug_bounds("playlist-loading-skeleton").is_some());
    assert!(visual.debug_bounds("song-empty-state").is_none());
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            assert_eq!(
                playlist_track_label(&probe.playlists, true),
                t!("Loading favorites…").to_string()
            );
            probe.fail();
            cx.notify();
        });
    });
    draw(&mut visual);
    assert!(visual.debug_bounds("playlist-loading-skeleton").is_none());
    assert!(visual.debug_bounds("song-empty-state").is_some());
    let retry = visual.debug_bounds("retry-playlist").unwrap();
    visual.simulate_click(retry.center(), Modifiers::default());
    draw(&mut visual);
    assert!(visual.debug_bounds("playlist-loading-skeleton").is_some());
    assert!(visual.debug_bounds("song-empty-state").is_none());
    assert!(visual.debug_bounds("retry-playlist").is_none());
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            assert!(probe.playlists.detail_loading());
            assert!(probe.playlists.detail_error().is_none());
            probe.playlists.apply_detail_update(
                probe.request,
                PlaylistUpdate::Complete(PlaylistProgress::default()),
            );
            assert!(probe.playlists.detail_ready());
            assert_eq!(
                playlist_track_label(&probe.playlists, true),
                t!("%{count} tracks", count = 0).to_string()
            );
            cx.notify();
        });
    });
    draw(&mut visual);
    assert!(visual.debug_bounds("playlist-loading-skeleton").is_none());
    assert!(visual.debug_bounds("song-empty-state").is_some());
}

#[gpui_pre::test]
fn retry_keeps_the_failed_preview_visible_until_a_new_prefix_replaces_it(cx: &mut TestAppContext) {
    let (probe, mut visual) = on_screen(cx);
    let mut old = None;
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            probe.publish(0, 32);
            old = Some(probe.request);
            probe.fail();
            assert_eq!(
                playlist_track_label(&probe.playlists, true),
                t!("%{count} tracks loaded", count = 32).to_string()
            );
            cx.notify();
        });
    });
    draw(&mut visual);
    assert!(visual.debug_bounds("preview-row-0").is_some());
    assert!(visual.debug_bounds("song-empty-state").is_none());
    let retry = visual.debug_bounds("retry-playlist").unwrap();
    visual.simulate_click(retry.center(), Modifiers::default());
    draw(&mut visual);
    assert!(visual.debug_bounds("preview-row-0").is_some());
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            assert_ne!(Some(probe.request), old);
            assert!(probe.playlists.detail_error().is_none());
            probe.publish(0, 4);
            assert_eq!(probe.playlists.songs().len(), 4);
            assert_eq!(probe.playlists.snapshot(), Some(probe.request));
            assert!(!probe.playlists.detail_ready());
            cx.notify();
        });
    });
    draw(&mut visual);
    assert!(visual.debug_bounds("preview-row-0").is_some());
    assert!(visual.debug_bounds("playlist-loading-skeleton").is_none());
}

#[gpui_pre::test]
fn appending_and_completing_a_preview_preserve_scroll_and_render_only_visible_rows(
    cx: &mut TestAppContext,
) {
    const TOTAL: usize = 8192;
    let (probe, mut visual) = on_screen(cx);
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            probe.publish(0, 64);
            cx.notify();
        });
    });
    draw(&mut visual);
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            probe
                .scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.0), px(-5.0 * SONG_ROW_HEIGHT)));
            cx.notify();
        });
    });
    draw(&mut visual);
    let before = visual.debug_bounds("preview-row-5").unwrap();
    let mut offset = None;
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            offset = Some(probe.scroll.0.borrow().base_handle.offset());
            probe.publish(64, 128);
            cx.notify();
        });
    });
    draw(&mut visual);
    assert_eq!(visual.debug_bounds("preview-row-5").unwrap(), before);
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            assert_eq!(Some(probe.scroll.0.borrow().base_handle.offset()), offset);
            assert!(probe.rendered_rows < 128);
            assert_eq!(
                playlist_track_label(&probe.playlists, true),
                t!("%{count} tracks loaded", count = 128).to_string()
            );
            for start in (128..TOTAL).step_by(128) {
                probe.publish(start, (start + 128).min(TOTAL));
            }
            cx.notify();
        });
    });
    draw(&mut visual);
    assert_eq!(visual.debug_bounds("preview-row-5").unwrap(), before);
    visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            assert_eq!(Some(probe.scroll.0.borrow().base_handle.offset()), offset);
            assert!(probe.rendered_rows <= 16);
            assert_eq!(probe.playlists.songs().len(), TOTAL);
            probe.playlists.apply_detail_update(
                probe.request,
                PlaylistUpdate::Complete(PlaylistProgress {
                    songs: TOTAL,
                    ..Default::default()
                }),
            );
            cx.notify();
        });
    });
    draw(&mut visual);
    assert_eq!(visual.debug_bounds("preview-row-5").unwrap(), before);
    visual.update(|_, cx| {
        probe.read_with(cx, |probe, _| {
            assert_eq!(Some(probe.scroll.0.borrow().base_handle.offset()), offset);
            assert_eq!(
                playlist_track_label(&probe.playlists, true),
                t!("%{count} tracks", count = TOTAL).to_string()
            );
        });
    });
}
