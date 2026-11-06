use super::*;
use crate::models::{QueueOrigin, Song};
use crate::recommendation::{RecommendationReason, RecommendedTrack};

const DAY: &str = "2026-11-04";
const NEXT_DAY: &str = "2026-11-05";

fn snapshot(server_id: &str, day: &str, variant: u32, ids: &[&str]) -> DailyMixSnapshot {
    DailyMixSnapshot {
        server_id: server_id.to_string(),
        day: day.to_string(),
        algorithm_version: 2,
        variant,
        generated_at: 0,
        tracks: ids
            .iter()
            .map(|id| RecommendedTrack {
                song: Song {
                    id: id.to_string(),
                    ..Song::default()
                },
                reason: RecommendationReason::Explore,
                score: 1.0,
                origin: QueueOrigin::DailyMix,
            })
            .collect(),
    }
}

fn recent(state: &DailyMixState) -> HashSet<&str> {
    state.recent_song_ids().map(String::as_str).collect()
}

#[test]
fn current_cache_restores_tracks_without_playback_or_loading() {
    let mut state = DailyMixState::default();
    state.restore(Some(snapshot("server", DAY, 3, &["a", "b"])), "server", DAY);
    assert_eq!(state.snapshot().unwrap().variant, 3);
    assert_eq!(recent(&state), HashSet::from(["a", "b"]));
    assert!(!state.loading());
    assert!(state.error().is_none());
    assert!(state.pending.is_none());
}

#[test]
fn stale_date_or_algorithm_cache_only_supplies_previous_tracks() {
    for old_algorithm in [true, false] {
        let mut state = DailyMixState::default();
        let mut cached = snapshot("server", DAY, 3, &["a", "b"]);
        if old_algorithm {
            cached.algorithm_version = 1;
        } else {
            cached.day = "2026-11-03".into();
        }
        state.restore(Some(cached), "server", DAY);
        assert!(state.snapshot().is_none());
        assert_eq!(recent(&state), HashSet::from(["a", "b"]));
        assert!(!state.loading());
        assert_eq!(state.refresh_variant("server", DAY), 0);
    }
}

#[test]
fn restoring_ignores_another_servers_cache_and_invalidates_pending_requests() {
    let mut requests = LibraryRequests::default();
    let mut state = DailyMixState::default();
    let old = state.begin("server", DAY, 0, true, &mut requests).unwrap();
    state.restore(
        Some(snapshot("other", DAY, 0, &["other-song"])),
        "server",
        DAY,
    );
    assert!(state.snapshot().is_none());
    assert!(recent(&state).is_empty());
    assert!(!state.loading());
    assert_eq!(
        state.complete(&old, Ok(snapshot("server", DAY, 0, &["a"]))),
        None
    );
}

#[test]
fn repeated_generation_coalesces_and_preserves_new_playback_demand() {
    let mut requests = LibraryRequests::default();
    let mut state = DailyMixState::default();
    let request = state.begin("server", DAY, 1, false, &mut requests).unwrap();
    assert!(state.begin("server", DAY, 1, true, &mut requests).is_none());
    assert!(state
        .begin("server", DAY, 2, false, &mut requests)
        .is_none());
    assert!(state.loading());
    assert_eq!(
        state.complete(&request, Ok(snapshot("server", DAY, 1, &["a"]))),
        Some(Ok(true))
    );
    assert!(!state.loading());
    assert_eq!(state.snapshot().unwrap().variant, 1);
    assert_eq!(
        state.complete(&request, Err("duplicate failure".into())),
        None
    );
    assert_eq!(
        state.complete(&request, Ok(snapshot("server", DAY, 1, &["duplicate"]))),
        None
    );
    assert_eq!(recent(&state), HashSet::from(["a"]));
    assert!(state.error().is_none());
}

#[test]
fn manual_queue_action_revokes_playback_but_keeps_generation_and_cache_update() {
    let mut requests = LibraryRequests::default();
    let mut state = DailyMixState::default();
    let request = state.begin("server", DAY, 0, true, &mut requests).unwrap();
    state.cancel_playback();
    assert!(state.loading());
    assert_eq!(
        state.complete(&request, Ok(snapshot("server", DAY, 0, &["a"]))),
        Some(Ok(false))
    );
    assert_eq!(recent(&state), HashSet::from(["a"]));
    let next = state.begin("server", DAY, 1, true, &mut requests).unwrap();
    state.cancel_playback();
    assert!(state.begin("server", DAY, 1, true, &mut requests).is_none());
    assert_eq!(
        state.complete(&next, Ok(snapshot("server", DAY, 1, &["b"]))),
        Some(Ok(true))
    );
}

#[test]
fn failed_refresh_keeps_snapshot_and_retry_clears_error_without_replaying_old_intent() {
    let mut requests = LibraryRequests::default();
    let mut state = DailyMixState::default();
    state.restore(Some(snapshot("server", DAY, 2, &["old"])), "server", DAY);
    let failed = state.begin("server", DAY, 3, true, &mut requests).unwrap();
    assert_eq!(
        state.complete(&failed, Err("offline".into())),
        Some(Err("offline"))
    );
    assert!(!state.loading());
    assert_eq!(state.error(), Some("offline"));
    assert_eq!(state.snapshot().unwrap().variant, 2);
    assert_eq!(recent(&state), HashSet::from(["old"]));
    assert_eq!(
        state.complete(&failed, Ok(snapshot("server", DAY, 3, &["late"]))),
        None
    );
    assert_eq!(state.error(), Some("offline"));
    let retry = state.begin("server", DAY, 2, false, &mut requests).unwrap();
    assert!(state.error().is_none());
    assert_eq!(state.complete(&failed, Err("old failure".into())), None);
    assert_eq!(
        state.complete(&retry, Ok(snapshot("server", DAY, 2, &["new"]))),
        Some(Ok(false))
    );
}

#[test]
fn new_day_supersedes_pending_generation_and_old_results_cannot_consume_playback_intent() {
    let mut requests = LibraryRequests::default();
    let mut state = DailyMixState::default();
    let old = state.begin("server", DAY, 0, false, &mut requests).unwrap();
    let current = state
        .begin("server", NEXT_DAY, 0, true, &mut requests)
        .unwrap();
    assert_eq!(
        state.complete(&old, Ok(snapshot("server", DAY, 0, &["old"]))),
        None
    );
    assert_eq!(state.complete(&old, Err("yesterday failure".into())), None);
    assert!(state.loading());
    assert!(state.snapshot().is_none());
    assert_eq!(
        state.complete(&current, Ok(snapshot("server", NEXT_DAY, 0, &["new"]))),
        Some(Ok(true))
    );
    assert_eq!(state.snapshot().unwrap().day, NEXT_DAY);
}

#[test]
fn server_reset_rejects_old_results_after_reconnecting_to_the_same_server() {
    let mut requests = LibraryRequests::default();
    let mut state = crate::app::AppState::default();
    let old = state
        .daily_mix
        .begin("server", DAY, 0, true, &mut requests)
        .unwrap();
    state = crate::app::AppState::default();
    let current = state
        .daily_mix
        .begin("server", DAY, 0, false, &mut requests)
        .unwrap();
    assert_eq!(
        state
            .daily_mix
            .complete(&old, Ok(snapshot("server", DAY, 0, &["old"]))),
        None
    );
    assert_eq!(
        state.daily_mix.complete(&old, Err("old failure".into())),
        None
    );
    assert!(state.daily_mix.loading());
    assert_eq!(
        state
            .daily_mix
            .complete(&current, Ok(snapshot("server", DAY, 0, &["new"]))),
        Some(Ok(false))
    );
}

#[test]
fn expiration_preserves_previous_ids_and_variant_changes_follow_the_day() {
    let mut state = DailyMixState::default();
    state.restore(Some(snapshot("server", DAY, 7, &["a", "b"])), "server", DAY);
    assert_eq!(state.refresh_variant("server", DAY), 8);
    assert_eq!(state.refresh_variant("server", NEXT_DAY), 0);
    assert_eq!(state.retry_variant(), 7);
    state.expire("server", DAY);
    assert!(state.snapshot().is_some());
    state.expire("server", NEXT_DAY);
    assert!(state.snapshot().is_none());
    assert_eq!(state.retry_variant(), 0);
    assert_eq!(recent(&state), HashSet::from(["a", "b"]));
}

#[test]
fn replacement_remembers_previous_mix_without_accumulating_all_past_tracks() {
    let mut requests = LibraryRequests::default();
    let mut state = DailyMixState::default();
    state.restore(Some(snapshot("server", DAY, 0, &["a", "b"])), "server", DAY);
    let first = state.begin("server", DAY, 1, false, &mut requests).unwrap();
    state.complete(&first, Ok(snapshot("server", DAY, 1, &["c"])));
    assert_eq!(recent(&state), HashSet::from(["a", "b", "c"]));
    let second = state.begin("server", DAY, 2, false, &mut requests).unwrap();
    state.complete(&second, Ok(snapshot("server", DAY, 2, &["d"])));
    assert_eq!(recent(&state), HashSet::from(["c", "d"]));
}

#[test]
fn empty_success_is_a_cached_snapshot_and_playback_intent_is_consumed_once() {
    let mut requests = LibraryRequests::default();
    let mut state = DailyMixState::default();
    let request = state.begin("server", DAY, 0, true, &mut requests).unwrap();
    assert_eq!(
        state.complete(&request, Ok(snapshot("server", DAY, 0, &[]))),
        Some(Ok(true))
    );
    assert!(state.snapshot().unwrap().tracks.is_empty());
    assert_eq!(
        state.complete(&request, Ok(snapshot("server", DAY, 0, &["late"]))),
        None
    );
    assert!(state.error().is_none());
    assert!(!state.loading());
}
