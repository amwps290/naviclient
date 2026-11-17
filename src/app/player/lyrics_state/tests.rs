use gpui_kit::px;

use super::*;
use crate::models::LyricLine;

fn media(id: &str) -> MediaKey {
    MediaKey::new(
        crate::source::SourceId::new(crate::source::ProviderKind::OpenSubsonic, "fixture"),
        id,
    )
}

fn lyrics(text: &str) -> Lyrics {
    Lyrics {
        lines: vec![LyricLine {
            text: text.to_string(),
            start_ms: Some(0),
            translation: None,
            ..Default::default()
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
    assert!(state.follow_reset);
}

#[test]
fn next_song_intro_requests_viewport_reset_even_without_an_active_cue() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let request = state.begin(&media("outro"), &mut requests).unwrap();
    state.complete(request, Ok(lyrics("last line")));
    set_follow(&mut state);
    state.follow_reset = false;
    let request = state.begin(&media("intro"), &mut requests).unwrap();
    assert_follow_cleared(&state);
    // Simulate the loading frame consuming the first reset.
    state.follow_reset = false;
    let mut intro = lyrics("first line");
    intro.lines[0].start_ms = Some(15_000);
    state.complete(request, Ok(intro));
    assert_follow_cleared(&state);
    assert_eq!(
        state
            .lyrics()
            .unwrap()
            .display_line_index(std::time::Duration::ZERO),
        None
    );
}

#[test]
fn same_song_coalesces_pending_and_cached_loads_and_resets_follow() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let request = state.begin(&media("a"), &mut requests).unwrap();
    set_follow(&mut state);
    assert_eq!(state.begin(&media("a"), &mut requests), None);
    assert!(state.loading());
    assert_follow_cleared(&state);
    assert!(state.complete(request, Ok(lyrics("current"))));
    set_follow(&mut state);
    assert_eq!(state.begin(&media("a"), &mut requests), None);
    assert!(!state.loading());
    assert_eq!(state.lyrics(), Some(&lyrics("current")));
    assert_follow_cleared(&state);
}

#[test]
fn empty_success_is_cached_without_repeated_requests() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let request = state.begin(&media("a"), &mut requests).unwrap();
    assert!(state.complete(request, Ok(Lyrics::default())));
    assert_eq!(state.begin(&media("a"), &mut requests), None);
    assert_eq!(state.lyrics(), Some(&Lyrics::default()));
    assert!(state.error().is_none());
}

#[test]
fn failed_load_can_retry_and_duplicate_results_cannot_replace_it() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let failed = state.begin(&media("a"), &mut requests).unwrap();
    assert!(state.complete(failed, Err("offline".into())));
    assert!(!state.loading());
    assert_eq!(state.error(), Some("offline"));
    assert!(!state.complete(failed, Ok(lyrics("duplicate"))));
    assert_eq!(state.error(), Some("offline"));
    let retry = state.begin(&media("a"), &mut requests).unwrap();
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
    let old_a = state.begin(&media("a"), &mut requests).unwrap();
    let b = state.begin(&media("b"), &mut requests).unwrap();
    let current_a = state.begin(&media("a"), &mut requests).unwrap();
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
    let a = state.begin(&media("a"), &mut requests).unwrap();
    state.complete(a, Ok(lyrics("a")));
    set_follow(&mut state);
    let b = state.begin(&media("b"), &mut requests).unwrap();
    assert!(state.lyrics().is_none());
    assert_follow_cleared(&state);
    assert!(state.complete(b, Ok(lyrics("b"))));
    assert_eq!(state.lyrics(), Some(&lyrics("b")));
}

#[test]
fn stopping_invalidates_pending_lyrics_even_when_the_song_is_restarted() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let old = state.begin(&media("a"), &mut requests).unwrap();
    set_follow(&mut state);
    state.clear();
    assert_follow_cleared(&state);
    assert!(!state.loading());
    assert!(!state.complete(old, Ok(lyrics("stopped"))));
    let current = state.begin(&media("a"), &mut requests).unwrap();
    assert!(!state.complete(old, Err("stopped error".into())));
    assert!(state.loading());
    assert!(state.complete(current, Ok(lyrics("restarted"))));
}

#[test]
fn server_reset_rejects_old_lyrics_with_reused_song_ids() {
    let mut requests = LibraryRequests::default();
    let mut state = crate::app::AppState::default();
    let old = state.lyrics.begin(&media("a"), &mut requests).unwrap();
    state = crate::app::AppState::default();
    let current = state.lyrics.begin(&media("a"), &mut requests).unwrap();
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
    let old = state.begin(&media("a"), &mut requests).unwrap();
    let current = state.begin(&media("b"), &mut requests).unwrap();
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

#[test]
fn same_song_id_from_another_source_requests_new_lyrics_and_rejects_old_reply() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let a = media("same");
    let b = MediaKey::new(
        crate::source::SourceId::new(crate::source::ProviderKind::OpenSubsonic, "other"),
        "same",
    );
    let old = state.begin(&a, &mut requests).unwrap();
    let current = state.begin(&b, &mut requests).unwrap();
    assert!(!state.complete(old, Ok(lyrics("other source"))));
    assert!(state.loading());
    assert!(state.complete(current, Ok(lyrics("current source"))));
    assert_eq!(state.lyrics(), Some(&lyrics("current source")));
    assert!(state.begin(&a, &mut requests).is_some());
}

#[test]
fn refresh_preserves_content_and_failed_refresh_does_not_erase_it() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let first = state.begin(&media("a"), &mut requests).unwrap();
    assert!(state.finish(
        first,
        LoadOutcome::ready(Document::native(lyrics("current")))
    ));
    let refresh = state.refresh(&media("a"), &mut requests);
    assert_eq!(state.lyrics(), Some(&lyrics("current")));
    assert!(state.finish(refresh, LoadOutcome::empty(LyricsStatus::Unavailable)));
    assert_eq!(state.lyrics(), Some(&lyrics("current")));
    assert_eq!(state.warning, Some(LyricsStatus::Unavailable));
}

#[test]
fn manual_application_rejects_pending_automatic_response() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let request = state.begin(&media("a"), &mut requests).unwrap();
    let mut document = Document::native(lyrics("chosen"));
    document.manual = true;
    document.temporary = true;
    state.apply(document);
    assert!(!state.finish(
        request,
        LoadOutcome::ready(Document::native(lyrics("late")))
    ));
    assert_eq!(state.lyrics(), Some(&lyrics("chosen")));
    assert!(state.document.as_ref().unwrap().temporary);
}

#[test]
fn highlighting_and_seek_share_the_adjusted_timestamp_and_plain_mode_removes_it() {
    let mut document = Document::native(lyrics("test"));
    document.lyrics.lines[0].start_ms = Some(1200);
    document.offset_ms = 500;
    let displayed = document.display_lyrics();
    let seek = displayed.lines[0].start_ms.unwrap();
    assert_eq!(seek, 1700);
    assert_eq!(
        displayed.active_line_index(std::time::Duration::from_millis(seek - 1)),
        None
    );
    assert_eq!(
        displayed.active_line_index(std::time::Duration::from_millis(seek)),
        Some(0)
    );
    document.offset_ms = -2000;
    assert_eq!(document.display_lyrics().lines[0].start_ms, Some(0));
    document.plain = true;
    assert!(!document.display_lyrics().is_synced());
    assert_eq!(document.display_lyrics().lines[0].start_ms, None);
}

#[tokio::test]
async fn stopping_aborts_the_pending_network_future() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    state.begin(&media("a"), &mut requests).unwrap();
    let task = tokio::spawn(std::future::pending::<()>());
    let abort = task.abort_handle();
    state.task = Some(task);
    state.clear();
    tokio::task::yield_now().await;
    assert!(abort.is_finished());
}

#[test]
fn failed_refresh_preserves_the_instrumental_state() {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    state.begin(&media("a"), &mut requests).unwrap();
    let mut document = Document::native(Lyrics::default());
    document.instrumental = true;
    state.apply(document);
    let refresh = state.refresh(&media("a"), &mut requests);
    assert!(state.finish(refresh, LoadOutcome::empty(LyricsStatus::Unavailable)));
    assert_eq!(state.status, LyricsStatus::Instrumental);
    assert_eq!(state.warning, Some(LyricsStatus::Unavailable));
}

#[tokio::test]
async fn native_http_failure_reaches_player_error_and_refresh_keeps_existing_content() {
    use crate::lyrics::{
        cache::LyricsCache, lrclib::Lrclib, service::LyricsService, LyricsPolicy, LyricsSettings,
    };
    use crate::test_support::http::Server;
    let directory = tempfile::tempdir().unwrap();
    let server = Server::with_handler(|_| (500, "failed".into()));
    let online = Server::new(vec![]);
    let source = crate::source::MusicSource::opensubsonic_with_route(
        "fixture",
        &server.url(""),
        "user",
        "password",
        crate::models::network::NetworkRoute::Direct,
    )
    .unwrap();
    let song = crate::models::Song {
        id: "a".into(),
        ..Default::default()
    };
    let mut requests = LibraryRequests::default();
    for settings in [
        LyricsSettings::default(),
        LyricsSettings {
            enabled: true,
            policy: LyricsPolicy::Manual,
            ..Default::default()
        },
    ] {
        let service = LyricsService {
            settings,
            provider: Ok(vec![crate::lyrics::online::Provider::Lrclib(
                Lrclib::test_client(online.url("")),
            )]),
            cache: LyricsCache::at(directory.path().join("cache.json")),
        };
        let mut state = LyricsState::default();
        let first = state.begin(&media("a"), &mut requests).unwrap();
        let outcome = service
            .load(
                &song,
                &media("a"),
                Some(&source),
                crate::models::CapabilitySupport::Unsupported,
                false,
                |_, _| {},
            )
            .await;
        assert!(state.finish(first, outcome));
        assert!(!state.loading());
        assert_eq!(state.status, LyricsStatus::SourceUnavailable);
        assert!(state.error().unwrap().contains("500"));
        assert!(state.begin(&media("a"), &mut requests).is_some());
        state.apply(Document::native(lyrics("current")));
        let refresh = state.refresh(&media("a"), &mut requests);
        let outcome = service
            .load(
                &song,
                &media("a"),
                Some(&source),
                crate::models::CapabilitySupport::Unsupported,
                true,
                |_, _| {},
            )
            .await;
        assert!(state.finish(refresh, outcome));
        assert_eq!(state.lyrics(), Some(&lyrics("current")));
        assert_eq!(state.status, LyricsStatus::Ready);
        assert_eq!(state.warning, Some(LyricsStatus::SourceUnavailable));
        assert!(state.error().is_none());
    }
    assert!(online.requests().is_empty());
}

fn online_document(id: u64) -> Document {
    Document::online(crate::lyrics::provider::Record {
        id,
        track_name: "歌曲".into(),
        artist_name: "歌手".into(),
        duration: 223.0,
        synced_lyrics: Some("[00:01.00]first\n[00:05.00]second".into()),
        ..Default::default()
    })
}

fn cache(directory: &tempfile::TempDir) -> crate::lyrics::cache::LyricsCache {
    crate::lyrics::cache::LyricsCache::at(directory.path().join("cache.json"))
}

fn showing(document: Document) -> LyricsState {
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    let request = state.begin(&media("a"), &mut requests).unwrap();
    assert!(state.finish(request, LoadOutcome::ready(document)));
    // The first frame after a load consumes the viewport reset.
    state.follow_reset = false;
    state
}

#[test]
fn timing_change_shows_at_once_and_is_remembered_without_pinning_automatic_lyrics() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut state = showing(online_document(1));
    state
        .change_timing(&cache, |document| document.offset_ms = 300)
        .unwrap();
    assert_eq!(state.lyrics().unwrap().lines[0].start_ms, Some(1300));
    assert_eq!(state.document.as_ref().unwrap().offset_ms, 300);
    assert_eq!(cache.preference(&media("a")).offset_ms, 300);
    // The automatic match may still be replaced by a better one later.
    assert!(cache.selected(&media("a")).is_none());
    assert_eq!(cache.preference(&media("b")).offset_ms, 0);
}

#[test]
fn timing_change_keeps_a_remembered_selection_pinned() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut document = online_document(7);
    document.manual = true;
    let mut state = showing(document);
    state
        .change_timing(&cache, |document| document.offset_ms = -400)
        .unwrap();
    let pin = cache.selected(&media("a")).unwrap();
    assert_eq!(pin.record.unwrap().id, 7);
    assert_eq!(pin.offset_ms, -400);
}

#[test]
fn session_only_selection_is_retimed_without_touching_the_remembered_offset() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut remembered = online_document(1);
    remembered.manual = true;
    remembered.offset_ms = -600;
    cache.remember(&media("a"), &remembered).unwrap();
    let mut session = online_document(2);
    session.manual = true;
    session.temporary = true;
    let mut state = showing(session);
    state
        .change_timing(&cache, |document| document.offset_ms = 300)
        .unwrap();
    assert_eq!(state.lyrics().unwrap().lines[0].start_ms, Some(1300));
    let pin = cache.selected(&media("a")).unwrap();
    assert_eq!(pin.record.unwrap().id, 1);
    assert_eq!(pin.offset_ms, -600);
}

#[test]
fn timing_change_leaves_the_viewport_where_the_listener_is_reading() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut state = showing(online_document(1));
    set_follow(&mut state);
    state
        .change_timing(&cache, |document| document.offset_ms = 300)
        .unwrap();
    assert_eq!(state.active_index, Some(2));
    assert_eq!(state.scroll_target, Some(px(-120.0)));
    assert!(!state.follow_reset);
}

#[test]
fn display_change_restarts_following_from_the_top() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut state = showing(online_document(1));
    set_follow(&mut state);
    state
        .change_display(&cache, |document| document.plain = true)
        .unwrap();
    assert!(!state.lyrics().unwrap().is_synced());
    assert!(cache.preference(&media("a")).plain);
    assert_follow_cleared(&state);
}

#[test]
fn timing_change_rejects_a_refresh_that_started_with_the_old_offset() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut requests = LibraryRequests::default();
    let mut state = showing(online_document(1));
    let refresh = state.refresh(&media("a"), &mut requests);
    state
        .change_timing(&cache, |document| document.offset_ms = 300)
        .unwrap();
    assert!(!state.loading());
    assert!(!state.finish(refresh, LoadOutcome::ready(online_document(1))));
    assert_eq!(state.document.as_ref().unwrap().offset_ms, 300);
}

#[test]
fn changes_without_a_document_save_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let mut requests = LibraryRequests::default();
    let mut state = LyricsState::default();
    state.begin(&media("a"), &mut requests).unwrap();
    state
        .change_timing(&cache, |document| document.offset_ms = 300)
        .unwrap();
    assert!(state.document.is_none());
    assert!(cache.preference(&media("a")).media.is_none());
}
