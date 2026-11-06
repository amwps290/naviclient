use gpui_kit::px;

use super::*;
use crate::models::LyricLine;

fn lyrics(text: &str) -> Lyrics {
    Lyrics {
        lines: vec![LyricLine {
            text: text.to_string(),
            start_ms: Some(0),
            translation: None,
        }],
        ..Lyrics::default()
    }
}

fn set_follow(state: &mut LyricsState) {
    state.active_index = Some(2);
    state.scroll_target = Some(px(-120.0));
}

fn assert_follow_cleared(state: &LyricsState) {
    assert_eq!(state.active_index, None);
    assert_eq!(state.scroll_target, None);
}

#[test]
fn same_song_coalesces_pending_and_cached_loads_and_resets_follow() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let request = state.begin("a", &mut requests).unwrap();
    set_follow(&mut state);
    assert_eq!(state.begin("a", &mut requests), None);
    assert!(state.loading());
    assert_follow_cleared(&state);
    assert!(state.complete(request, Ok(lyrics("current"))));
    set_follow(&mut state);
    assert_eq!(state.begin("a", &mut requests), None);
    assert!(!state.loading());
    assert_eq!(state.lyrics(), Some(&lyrics("current")));
    assert_follow_cleared(&state);
}

#[test]
fn empty_success_is_cached_without_repeated_requests() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let request = state.begin("a", &mut requests).unwrap();
    assert!(state.complete(request, Ok(Lyrics::default())));
    assert_eq!(state.begin("a", &mut requests), None);
    assert_eq!(state.lyrics(), Some(&Lyrics::default()));
    assert!(state.error().is_none());
}

#[test]
fn failed_load_can_retry_and_duplicate_results_cannot_replace_it() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let failed = state.begin("a", &mut requests).unwrap();
    assert!(state.complete(failed, Err("offline".into())));
    assert!(!state.loading());
    assert_eq!(state.error(), Some("offline"));
    assert!(!state.complete(failed, Ok(lyrics("duplicate"))));
    assert_eq!(state.error(), Some("offline"));
    let retry = state.begin("a", &mut requests).unwrap();
    assert_ne!(failed, retry);
    assert!(state.error().is_none());
    assert!(state.loading());
    assert!(!state.complete(failed, Err("old error".into())));
    assert!(state.complete(retry, Ok(lyrics("retried"))));
    assert_eq!(state.lyrics(), Some(&lyrics("retried")));
}

#[test]
fn returning_to_a_song_rejects_its_previous_success_and_failure() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let old_a = state.begin("a", &mut requests).unwrap();
    let b = state.begin("b", &mut requests).unwrap();
    let current_a = state.begin("a", &mut requests).unwrap();
    assert!(!state.complete(old_a, Ok(lyrics("old a"))));
    assert!(!state.complete(old_a, Err("old a error".into())));
    assert!(!state.complete(b, Ok(lyrics("b"))));
    assert!(state.loading());
    assert!(state.lyrics().is_none());
    assert!(state.error().is_none());
    assert!(state.complete(current_a, Ok(lyrics("new a"))));
    assert_eq!(state.lyrics(), Some(&lyrics("new a")));
}

#[test]
fn changing_songs_discards_cached_lyrics_and_follow_state() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let a = state.begin("a", &mut requests).unwrap();
    state.complete(a, Ok(lyrics("a")));
    set_follow(&mut state);
    let b = state.begin("b", &mut requests).unwrap();
    assert!(state.lyrics().is_none());
    assert_follow_cleared(&state);
    assert!(state.complete(b, Ok(lyrics("b"))));
    assert_eq!(state.lyrics(), Some(&lyrics("b")));
}

#[test]
fn stopping_invalidates_pending_lyrics_even_when_the_song_is_restarted() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let old = state.begin("a", &mut requests).unwrap();
    set_follow(&mut state);
    state.clear();
    assert_follow_cleared(&state);
    assert!(!state.loading());
    assert!(!state.complete(old, Ok(lyrics("stopped"))));
    let current = state.begin("a", &mut requests).unwrap();
    assert!(!state.complete(old, Err("stopped error".into())));
    assert!(state.loading());
    assert!(state.complete(current, Ok(lyrics("restarted"))));
}

#[test]
fn server_reset_rejects_old_lyrics_with_reused_song_ids() {
    let mut requests = LibraryRequests::default();
    let mut state = crate::app::AppState::default();
    let old = state.lyrics.begin("a", &mut requests).unwrap();
    state = crate::app::AppState::default();
    let current = state.lyrics.begin("a", &mut requests).unwrap();
    assert!(!state.lyrics.complete(old, Ok(lyrics("old server"))));
    assert!(!state.lyrics.complete(old, Err("old server error".into())));
    assert!(state.lyrics.loading());
    assert!(state.lyrics.complete(current, Ok(lyrics("new server"))));
    assert_eq!(state.lyrics.lyrics(), Some(&lyrics("new server")));
}

#[test]
fn stale_and_duplicate_results_preserve_the_newer_follow_target() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let old = state.begin("a", &mut requests).unwrap();
    let current = state.begin("b", &mut requests).unwrap();
    set_follow(&mut state);
    assert!(!state.complete(old, Err("stale".into())));
    assert_eq!(state.active_index, Some(2));
    assert_eq!(state.scroll_target, Some(px(-120.0)));
    assert!(state.complete(current, Ok(lyrics("current"))));
    assert_follow_cleared(&state);
    set_follow(&mut state);
    assert!(!state.complete(current, Ok(lyrics("duplicate"))));
    assert_eq!(state.active_index, Some(2));
    assert_eq!(state.scroll_target, Some(px(-120.0)));
    assert_eq!(state.lyrics(), Some(&lyrics("current")));
}
