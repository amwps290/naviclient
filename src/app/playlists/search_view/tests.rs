use super::*;
use gpui_kit::component::Root;
use gpui_kit::{TestAppContext, VisualTestContext};

struct SearchProbe {
    input: Entity<InputState>,
    list_focus: gpui_kit::FocusHandle,
    changes: usize,
    enters: usize,
    subscription: Option<Subscription>,
    playlists: PlaylistLibrary,
}

impl SearchProbe {
    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.set_query(String::new());
        self.playlists.search.expanded = false;
        self.list_focus.focus(window, cx);
        cx.notify();
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.playlists.search.expanded {
            self.close_search(window, cx);
        } else {
            self.playlists.search.expanded = true;
            self.input.update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
        }
    }

    fn set_query(&mut self, query: String) {
        self.playlists.search.set_query(query);
        if let Some((request, index, terms)) = self.playlists.search.start() {
            let indices = index
                .matching(
                    &terms,
                    &crate::app::playlists::search_index::Cancellation::default(),
                )
                .unwrap();
            self.playlists.search.complete(request, Ok(indices));
        }
    }
}

impl Render for SearchProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(320.0))
            .h(px(400.0))
            .child(
                h_flex()
                    .w_full()
                    .h(px(SEARCH_HEIGHT))
                    .child(render_playlist_search_controls(
                        &self.playlists,
                        &self.input,
                        self.input.read(cx).focus_handle(cx).is_focused(window),
                        cx.listener(|this, _, window, cx| this.toggle_search(window, cx)),
                        cx.listener(|this, _: &Escape, window, cx| {
                            if this.input.read(cx).value().is_empty() {
                                this.close_search(window, cx);
                            } else {
                                this.input
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                this.set_query(String::new());
                                this.changes += 1;
                            }
                        }),
                        cx.listener(|this, _, window, cx| this.close_search(window, cx)),
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .h(px(24.0))
                    .when(self.playlists.search.expanded, |row| {
                        row.child(render_playlist_search_feedback(
                            &self.playlists,
                            cx.listener(|this, _, _, cx| {
                                this.playlists.search.retry();
                                cx.notify();
                            }),
                            cx,
                        ))
                    }),
            )
            .child(
                div()
                    .id("search-probe-list")
                    .debug_selector(|| "search-probe-list".into())
                    .track_focus(&self.list_focus)
                    .tab_index(0)
                    .flex_1()
                    .child(if self.playlists.search.filtered() {
                        render_playlist_search_empty(&self.playlists, cx)
                    } else {
                        div().into_any_element()
                    }),
            )
    }
}

fn harness(cx: &mut TestAppContext) -> (Entity<SearchProbe>, VisualTestContext) {
    harness_with_expanded(cx, true)
}

fn harness_with_expanded(
    cx: &mut TestAppContext,
    expanded: bool,
) -> (Entity<SearchProbe>, VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|cx| {
                let input = cx.new(|cx| InputState::new(window, cx));
                let mut playlists = PlaylistLibrary::default();
                let mut requests = LibraryRequests::default();
                let playlist = Playlist {
                    id: "p".into(),
                    song_count: Some(999),
                    ..Default::default()
                };
                playlists.select(playlist.clone());
                let request = playlists.begin_detail(&mut requests).unwrap();
                playlists.complete_detail(
                    request,
                    Ok(crate::playlist::PlaylistDetails {
                        playlist,
                        unavailable_count: 0,
                        songs: ["A", "love story", "A"]
                            .map(|title| Song {
                                id: title.into(),
                                title: title.into(),
                                ..Default::default()
                            })
                            .to_vec(),
                    }),
                );
                playlists.search.expanded = expanded;
                let mut probe = SearchProbe {
                    input: input.clone(),
                    playlists,
                    list_focus: cx.focus_handle().tab_stop(true),
                    changes: 0,
                    enters: 0,
                    subscription: None,
                };
                probe.subscription = Some(cx.subscribe(
                    &input,
                    |this: &mut SearchProbe, input, event, cx| match event {
                        InputEvent::Change => {
                            this.changes += 1;
                            this.set_query(input.read(cx).value().to_string());
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.enters += 1,
                        InputEvent::Focus | InputEvent::Blur => cx.notify(),
                    },
                ));
                probe
            });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let probe = probe.unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw(&mut visual);
    if expanded {
        visual.update(|window, cx| {
            probe
                .read(cx)
                .input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx)
        });
    }
    draw(&mut visual);
    (probe, visual)
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

#[gpui_pre::test]
fn search_icon_expands_and_focuses_input_then_collapses_and_clears_filter(cx: &mut TestAppContext) {
    let (probe, mut visual) = harness_with_expanded(cx, false);
    assert!(visual.debug_bounds("playlist-search-bar").is_none());
    assert!(visual.debug_bounds("playlist-search-count").is_none());
    let icon = visual.debug_bounds("toggle-playlist-search").unwrap();
    visual.simulate_click(icon.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    assert!(visual.debug_bounds("playlist-search-bar").is_some());
    visual.update(|window, cx| {
        assert!(probe.read(cx).playlists.search.expanded);
        assert!(probe
            .read(cx)
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window));
        assert!(window.has_focused_input(cx));
    });
    visual.simulate_keystrokes("l o v e");
    draw(&mut visual);
    visual.update(|_, cx| assert!(probe.read(cx).playlists.search.filtered()));
    let icon = visual.debug_bounds("toggle-playlist-search").unwrap();
    visual.simulate_click(icon.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    assert!(visual.debug_bounds("playlist-search-bar").is_none());
    visual.update(|window, cx| {
        assert!(!probe.read(cx).playlists.search.expanded);
        assert!(!probe.read(cx).playlists.search.filtered());
        assert!(probe.read(cx).input.read(cx).value().is_empty());
        assert!(probe.read(cx).list_focus.is_focused(window));
    });
    let icon = visual.debug_bounds("toggle-playlist-search").unwrap();
    visual.simulate_click(icon.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    visual.update(|window, cx| {
        assert!(probe.read(cx).playlists.search.expanded);
        assert!(probe.read(cx).input.read(cx).value().is_empty());
        assert!(probe
            .read(cx)
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window));
    });
}

#[gpui_pre::test]
fn input_reveals_to_the_right_without_moving_icon_or_song_list(cx: &mut TestAppContext) {
    let (probe, mut visual) = harness_with_expanded(cx, false);
    let icon = visual.debug_bounds("toggle-playlist-search").unwrap();
    let list = visual.debug_bounds("search-probe-list").unwrap();
    assert_eq!(
        visual
            .debug_bounds("playlist-search-reveal")
            .unwrap()
            .size
            .width,
        px(SEARCH_HEIGHT),
    );
    visual.simulate_click(icon.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    let expanded_icon = visual.debug_bounds("toggle-playlist-search").unwrap();
    let input = visual.debug_bounds("playlist-search-bar").unwrap();
    assert_eq!(expanded_icon, icon);
    assert!(input.origin.x >= icon.origin.x + icon.size.width);
    assert_eq!(input.center().y, icon.center().y);
    assert_eq!(
        visual
            .debug_bounds("playlist-search-reveal")
            .unwrap()
            .size
            .height,
        px(SEARCH_HEIGHT)
    );
    assert_eq!(visual.debug_bounds("search-probe-list").unwrap(), list);
    visual.simulate_click(icon.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    assert_eq!(visual.debug_bounds("toggle-playlist-search").unwrap(), icon);
    assert_eq!(visual.debug_bounds("search-probe-list").unwrap(), list);
    assert_eq!(
        visual
            .debug_bounds("playlist-search-reveal")
            .unwrap()
            .size
            .width,
        px(SEARCH_HEIGHT),
    );
    visual.update(|window, cx| {
        assert!(!probe.read(cx).playlists.search.expanded);
        assert!(!window.has_focused_input(cx));
    });
}

#[test]
fn horizontal_reveal_spring_grows_shrinks_and_reverses_continuously() {
    // GPUI's spring uses wall time, unlike the test executor's fake clock.
    // Test the exact rendering config with deterministic deltas, and test
    // actual input/icon geometry separately with reduced motion enabled.
    use gpui_kit::SpringState;
    let start = SpringState {
        position: 0.0,
        velocity: 0.0,
    };
    let first = SEARCH_REVEAL_SPRING.step(start, 1.0, 0.05);
    let second = SEARCH_REVEAL_SPRING.step(first, 1.0, 0.05);
    assert!(first.position > 0.0 && second.position > first.position && second.position < 1.0);
    let retargeted = SEARCH_REVEAL_SPRING.step(second, 0.0, 0.0);
    assert!((retargeted.position - second.position).abs() < 0.00001);
    assert!((retargeted.velocity - second.velocity).abs() < 0.00001);
    let closing = SEARCH_REVEAL_SPRING.step(retargeted, 0.0, 0.06);
    assert!(closing.position > 0.0 && closing.position < second.position);
    let reversed = SEARCH_REVEAL_SPRING.step(closing, 1.0, 0.0);
    assert!((reversed.position - closing.position).abs() < 0.00001);
    assert!((reversed.velocity - closing.velocity).abs() < 0.00001);
    let opened = SEARCH_REVEAL_SPRING.step(reversed, 1.0, 0.5);
    assert!((opened.position - 1.0).abs() < 0.001);
    let closed = SEARCH_REVEAL_SPRING.step(opened, 0.0, 0.5);
    assert!(closed.position.abs() < 0.001);
}

fn assert_search_input_frame(visual: &mut VisualTestContext, focused: bool) {
    let capsule_bounds = visual.debug_bounds("playlist-search-reveal").unwrap();
    let input_bounds = visual.debug_bounds("playlist-search-input").unwrap();
    visual.update(|window, cx| {
        let bounds = capsule_bounds.scale(window.scale_factor());
        let editor_bounds = input_bounds.scale(window.scale_factor());
        let quads = window.painted_quads();
        let frame = quads
            .iter()
            .find(|quad| {
                quad.bounds == bounds
                    && !quad.background.is_transparent()
                    && !quad.border_widths.any(|width| width.0 > 0.0)
            })
            .expect("icon, input and close must share one capsule background");
        let expected_radii = gpui_kit::Corners::all(gpui_kit::ScaledPixels(
            SEARCH_HEIGHT / 2.0 * window.scale_factor(),
        ));
        assert_eq!(frame.corner_radii, expected_radii);
        assert!(
            !quads
                .iter()
                .any(|quad| quad.bounds == editor_bounds && !quad.background.is_transparent()),
            "the embedded editor must not paint a separate input box"
        );
        let borders = quads
            .iter()
            .filter(|quad| {
                quad.border_widths.any(|width| width.0 > 0.0)
                    && !quad.border_color.is_transparent()
                    && quad.bounds.size.width.0 > bounds.size.width.0 * 0.8
                    && quad.bounds.size.height.0 < bounds.size.height.0 * 2.0
            })
            .collect::<Vec<_>>();
        if focused {
            assert!(
                !borders.is_empty(),
                "focus must retain a visible capsule border"
            );
            for border in borders {
                assert_eq!(
                    border.bounds, bounds,
                    "focus must be drawn inside the capsule, not outside its clip"
                );
                assert_eq!(border.corner_radii, expected_radii);
                assert_eq!(border.border_color, cx.theme().ring);
            }
        } else {
            assert!(
                borders.is_empty(),
                "unfocused search should have the reference's borderless capsule appearance"
            );
        }
    });
}

#[gpui_pre::test]
fn focus_uses_the_same_rounded_frame_without_a_clipped_outer_ring(cx: &mut TestAppContext) {
    use gpui_kit::component::{Theme, ThemeMode};
    let (probe, mut visual) = harness(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|_, cx| Theme::change(mode, None, cx));
        draw(&mut visual);
        assert_search_input_frame(&mut visual, true);
        visual.update(|window, cx| {
            let focus = probe.read(cx).list_focus.clone();
            focus.focus(window, cx);
        });
        draw(&mut visual);
        assert_search_input_frame(&mut visual, false);
        visual.update(|window, cx| {
            let input = probe.read(cx).input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        draw(&mut visual);
        assert_search_input_frame(&mut visual, true);
    }
}

#[gpui_pre::test]
fn empty_search_can_be_closed_without_entering_a_query(cx: &mut TestAppContext) {
    let (probe, mut visual) = harness(cx);
    let close = visual.debug_bounds("close-playlist-search").unwrap();
    visual.simulate_click(close.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    assert!(visual.debug_bounds("playlist-search-bar").is_none());
    visual.update(|window, cx| {
        assert!(!probe.read(cx).playlists.search.expanded);
        assert!(probe.read(cx).list_focus.is_focused(window));
    });
}

#[gpui_pre::test]
fn local_search_input_accepts_letters_spaces_and_chinese_without_swallowing_text(
    cx: &mut TestAppContext,
) {
    let (probe, mut visual) = harness(cx);
    visual.simulate_keystrokes("a space b");
    draw(&mut visual);
    visual.update(|window, cx| {
        assert_eq!(probe.read(cx).input.read(cx).value().as_ref(), "a b");
        assert!(window.has_focused_input(cx));
        let input = probe.read(cx).input.clone();
        input.update(cx, |input, cx| input.set_value("周杰伦 稻香", window, cx));
    });
    draw(&mut visual);
    visual.simulate_keystrokes("enter");
    draw(&mut visual);
    visual.update(|_, cx| {
        assert_eq!(
            probe.read(cx).input.read(cx).value().as_ref(),
            "周杰伦 稻香"
        );
        assert_eq!(
            probe.read(cx).enters,
            1,
            "Enter stays an input event instead of playing a row"
        );
        assert!(probe.read(cx).changes >= 3);
    });
}

#[gpui_pre::test]
fn zero_matches_show_actual_count_and_close_button_fits_a_narrow_search_bar(
    cx: &mut TestAppContext,
) {
    let (probe, mut visual) = harness(cx);
    visual.simulate_keystrokes("m i s s i n g");
    draw(&mut visual);
    visual.update(|_, cx| {
        assert_eq!(
            playlist_search_label(&probe.read(cx).playlists),
            t!("%{matched} / %{total} tracks", matched = 0, total = 3).to_string()
        );
    });
    assert!(visual.debug_bounds("song-empty-state").is_some());
    let input = visual.debug_bounds("playlist-search-input").unwrap();
    let clear = visual.debug_bounds("close-playlist-search").unwrap();
    let count = visual.debug_bounds("playlist-search-count").unwrap();
    assert!(input.origin.x + input.size.width <= clear.origin.x);
    assert!(count.origin.y >= input.origin.y + input.size.height);
    visual.simulate_click(clear.center(), gpui_kit::Modifiers::default());
    draw(&mut visual);
    visual.update(|window, cx| {
        assert!(probe.read(cx).input.read(cx).value().is_empty());
        assert!(!probe.read(cx).playlists.search.filtered());
        assert!(probe.read(cx).list_focus.is_focused(window));
        assert_eq!(
            playlist_search_label(&probe.read(cx).playlists),
            t!("%{count} tracks", count = 3).to_string()
        );
    });
}

#[test]
fn search_labels_distinguish_loading_pending_failure_and_zero_matches() {
    use crate::app::playlists::search_index::Cancellation;
    let mut playlists = PlaylistLibrary::default();
    let mut requests = LibraryRequests::default();
    let playlist = Playlist {
        id: "p".into(),
        ..Default::default()
    };
    playlists.select(playlist.clone());
    assert_eq!(
        playlist_search_label(&playlists),
        t!("Search available when loading completes").to_string()
    );
    let request = playlists.begin_detail(&mut requests).unwrap();
    playlists.complete_detail(
        request,
        Ok(crate::playlist::PlaylistDetails {
            playlist,
            songs: vec![Song {
                title: "A".into(),
                ..Default::default()
            }],
            unavailable_count: 0,
        }),
    );
    playlists.search.set_query("missing".into());
    assert_eq!(
        playlist_search_label(&playlists),
        t!("Filtering tracks…").to_string()
    );
    let (request, index, terms) = playlists.search.start().unwrap();
    playlists.search.complete(
        request,
        Ok(index.matching(&terms, &Cancellation::default()).unwrap()),
    );
    assert_eq!(
        playlist_search_label(&playlists),
        t!("%{matched} / %{total} tracks", matched = 0, total = 1).to_string()
    );
    playlists.search.set_query("a".into());
    let request = playlists.search.start().unwrap().0;
    playlists
        .search
        .complete(request, Err("worker stopped".into()));
    assert_eq!(
        playlist_search_label(&playlists),
        t!("Playlist search failed").to_string()
    );
}

#[gpui_pre::test]
fn tab_moves_from_input_to_close_button_then_list(cx: &mut TestAppContext) {
    let (probe, mut visual) = harness(cx);
    visual.simulate_keystrokes("a");
    draw(&mut visual);
    visual.simulate_keystrokes("tab");
    draw(&mut visual);
    visual.update(|window, cx| {
        assert!(!probe
            .read(cx)
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window));
        assert!(!probe.read(cx).list_focus.is_focused(window));
    });
    visual.simulate_keystrokes("tab");
    draw(&mut visual);
    visual.update(|window, cx| assert!(probe.read(cx).list_focus.is_focused(window)));
}

#[gpui_pre::test]
fn ctrl_a_selects_query_text_and_two_escapes_clear_then_return_to_list(cx: &mut TestAppContext) {
    let (probe, mut visual) = harness(cx);
    visual.simulate_keystrokes("l o v e space s t o r y");
    draw(&mut visual);
    visual.simulate_keystrokes("ctrl-a");
    draw(&mut visual);
    visual.update(|_, cx| {
        assert_eq!(
            probe.read(cx).input.read(cx).selected_value().as_ref(),
            "love story"
        )
    });
    visual.simulate_keystrokes("escape");
    draw(&mut visual);
    visual.update(|window, cx| {
        assert!(probe.read(cx).input.read(cx).value().is_empty());
        assert!(probe
            .read(cx)
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window));
    });
    visual.simulate_keystrokes("escape");
    draw(&mut visual);
    visual.update(|window, cx| {
        assert!(probe.read(cx).list_focus.is_focused(window));
        assert!(!probe.read(cx).playlists.search.expanded);
        assert!(!window.has_focused_input(cx));
    });
}
