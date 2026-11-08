use super::*;
use crate::models::{LyricLine, Lyrics};

const VIEWS: [View; 12] = [
    View::Home,
    View::Favorites,
    View::Artists,
    View::Albums,
    View::Genres,
    View::GenreDetail,
    View::Playlists,
    View::Search,
    View::ArtistDetail,
    View::AlbumDetail,
    View::PlaylistDetail,
    View::NowPlaying,
];

fn set_follow(state: &mut AppState) {
    state.lyrics.active_index = Some(2);
    state.lyrics.scroll_target = Some(px(-120.0));
}

fn assert_follow(state: &AppState, reset: bool) {
    assert_eq!(state.lyrics.active_index, (!reset).then_some(2));
    assert_eq!(state.lyrics.scroll_target, (!reset).then_some(px(-120.0)));
}

fn lyrics() -> Lyrics {
    Lyrics {
        lines: vec![LyricLine {
            text: "cached lyrics".into(),
            start_ms: Some(0),
            translation: None,
        }],
        ..Default::default()
    }
}

#[test]
fn browsing_navigation_closes_only_settings_and_preserves_drawer_selection_and_follow() {
    for view in VIEWS {
        let mut state = AppState {
            view: View::NowPlaying,
            view_before_now_playing: View::AlbumDetail,
            queue_drawer_open: true,
            queue_drawer_tab: QueueDrawerTab::History,
            ..Default::default()
        };
        state.settings.open = true;
        state.settings.section = SettingsSection::Shortcuts;
        state.settings.editing_server = Some("server".into());
        let recording = ShortcutRecording::Global(ShortcutAction::TogglePlayback);
        state.settings.start_recording(recording);
        state.song_selection.toggle(SongScope::History, 1, 3);
        let selection = state.song_selection.clone();
        set_follow(&mut state);

        state.navigate_to(view);

        assert_eq!(state.view, view);
        assert!(!state.settings.open);
        assert_eq!(state.settings.section, SettingsSection::Shortcuts);
        assert_eq!(state.settings.editing_server.as_deref(), Some("server"));
        assert_eq!(state.settings.recording(), Some(recording));
        assert!(state.queue_drawer_open);
        assert_eq!(state.queue_drawer_tab, QueueDrawerTab::History);
        assert_eq!(state.song_selection, selection);
        assert_eq!(state.view_before_now_playing, View::AlbumDetail);
        assert_follow(&state, false);
    }
}

#[test]
fn repeated_now_playing_entry_retains_the_original_return_page_and_pending_lyrics() {
    for view in VIEWS {
        let mut state = AppState {
            view,
            view_before_now_playing: View::Search,
            queue_drawer_open: true,
            ..Default::default()
        };
        let mut requests = LibraryRequests::default();
        let request = state
            .lyrics
            .begin(
                &crate::source::MediaKey::new(
                    crate::source::SourceId::new(
                        crate::source::ProviderKind::OpenSubsonic,
                        "fixture",
                    ),
                    "song",
                ),
                &mut requests,
            )
            .unwrap();
        let expected_return = if view == View::NowPlaying {
            View::Search
        } else {
            view
        };
        state.song_selection.toggle(SongScope::Queue, 2, 3);
        let selection = state.song_selection.clone();

        for _ in 0..3 {
            state.settings.open = true;
            set_follow(&mut state);
            state.open_now_playing();
            assert_eq!(state.view, View::NowPlaying);
            assert_eq!(state.view_before_now_playing, expected_return);
            assert!(!state.settings.open);
            assert!(state.queue_drawer_open);
            assert_eq!(state.song_selection, selection);
            assert!(state.lyrics.loading());
            assert_follow(&state, true);
        }

        state.leave_now_playing();
        assert_eq!(state.view, expected_return);
        assert!(state.lyrics.complete(request, Ok(lyrics())));
        assert_eq!(state.lyrics.lyrics(), Some(&lyrics()));
    }
}

#[test]
fn leaving_now_playing_restores_each_page_without_closing_settings_or_the_drawer() {
    for return_view in VIEWS {
        let mut state = AppState {
            view: View::NowPlaying,
            view_before_now_playing: return_view,
            queue_drawer_open: true,
            queue_drawer_tab: QueueDrawerTab::History,
            ..Default::default()
        };
        state.settings.open = true;
        state.song_selection.toggle(SongScope::History, 1, 3);
        let selection = state.song_selection.clone();
        set_follow(&mut state);

        state.leave_now_playing();

        assert_eq!(state.view, return_view);
        assert_eq!(state.view_before_now_playing, return_view);
        assert!(state.settings.open);
        assert!(state.queue_drawer_open);
        assert_eq!(state.queue_drawer_tab, QueueDrawerTab::History);
        assert_eq!(state.song_selection, selection);
        assert_follow(&state, true);
    }
}

#[test]
fn escape_closes_exactly_one_layer_for_every_view_scope_and_overlay_combination() {
    for view in VIEWS {
        for scope in SongScope::ALL {
            for mask in 0..8 {
                let selected = mask & 1 != 0;
                let settings = mask & 2 != 0;
                let drawer = mask & 4 != 0;
                let mut state = AppState {
                    view,
                    view_before_now_playing: View::GenreDetail,
                    queue_drawer_open: drawer,
                    queue_drawer_tab: QueueDrawerTab::History,
                    ..Default::default()
                };
                state.settings.open = settings;
                state.song_selection.select(scope, 1, 3);
                if selected {
                    state.song_selection.toggle(scope, 1, 3);
                }
                set_follow(&mut state);

                state.close_top_overlay();

                let navigating = !selected && !settings && !drawer;
                let expected_view = if navigating {
                    match view {
                        View::ArtistDetail => View::Artists,
                        View::AlbumDetail => View::Albums,
                        View::GenreDetail => View::Genres,
                        View::PlaylistDetail => View::Playlists,
                        View::NowPlaying => View::GenreDetail,
                        _ => view,
                    }
                } else {
                    view
                };
                assert_eq!(state.view, expected_view, "{view:?}, {scope:?}, {mask}");
                assert_eq!(state.settings.open, settings && selected);
                assert_eq!(state.queue_drawer_open, drawer && (selected || settings));
                assert!(state.song_selection.multi().is_none());
                assert_eq!(state.song_selection.cursor(), Some((scope, 1)));
                assert_eq!(state.view_before_now_playing, View::GenreDetail);
                assert_eq!(state.queue_drawer_tab, QueueDrawerTab::History);
                assert_follow(&state, navigating && view == View::NowPlaying);
            }
        }
    }
}

#[test]
fn successive_escape_events_unwind_selection_settings_drawer_player_and_detail_page() {
    let mut state = AppState {
        view: View::NowPlaying,
        view_before_now_playing: View::AlbumDetail,
        queue_drawer_open: true,
        ..Default::default()
    };
    state.settings.open = true;
    state.song_selection.toggle(SongScope::Queue, 1, 3);
    set_follow(&mut state);

    for (view, settings, drawer, reset_follow) in [
        (View::NowPlaying, true, true, false),
        (View::NowPlaying, false, true, false),
        (View::NowPlaying, false, false, false),
        (View::AlbumDetail, false, false, true),
        (View::Albums, false, false, true),
        (View::Albums, false, false, true),
    ] {
        state.close_top_overlay();
        assert_eq!(state.view, view);
        assert_eq!(state.settings.open, settings);
        assert_eq!(state.queue_drawer_open, drawer);
        assert!(state.song_selection.multi().is_none());
        assert_eq!(state.song_selection.cursor(), Some((SongScope::Queue, 1)));
        assert_follow(&state, reset_follow);
    }
}

#[test]
fn now_playing_transitions_reset_follow_without_discarding_cached_lyrics_or_errors() {
    for succeeded in [false, true] {
        let mut state = AppState::default();
        let mut requests = LibraryRequests::default();
        let request = state
            .lyrics
            .begin(
                &crate::source::MediaKey::new(
                    crate::source::SourceId::new(
                        crate::source::ProviderKind::OpenSubsonic,
                        "fixture",
                    ),
                    "song",
                ),
                &mut requests,
            )
            .unwrap();
        assert!(state.lyrics.complete(
            request,
            if succeeded {
                Ok(lyrics())
            } else {
                Err("offline".into())
            },
        ));

        for step in 0..3 {
            set_follow(&mut state);
            match step {
                0 => state.open_now_playing(),
                1 => state.leave_now_playing(),
                _ => {
                    state.open_now_playing();
                    set_follow(&mut state);
                    state.close_top_overlay();
                }
            }
            assert_follow(&state, true);
            assert!(!state.lyrics.loading());
            if succeeded {
                assert_eq!(state.lyrics.lyrics(), Some(&lyrics()));
                assert!(state.lyrics.error().is_none());
            } else {
                assert!(state.lyrics.lyrics().is_none());
                assert_eq!(state.lyrics.error(), Some("offline"));
            }
        }
    }
}
