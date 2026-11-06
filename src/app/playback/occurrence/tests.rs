use super::*;
use crate::models::{QueueEntry, QueueOrigin, Song};

fn entry(id: &str) -> QueueEntry {
    QueueEntry::new(
        Song {
            id: id.into(),
            ..Default::default()
        },
        QueueOrigin::Manual,
    )
}

#[test]
fn retry_preserves_identity_and_reporting_but_rearms_terminal_handling() {
    let entry = entry("song");
    let mut state = PlaybackOccurrence::default();
    assert!(state.select(entry.id(), "song", Selection::New));
    let id = state.id();
    assert!(state.claim_report("song"));
    assert!(state.claim_scrobble("song"));
    assert!(state.claim_terminal(state.id(), false));
    assert!(!state.claim_terminal(state.id(), false));
    assert!(!state.select(entry.id(), "song", Selection::Resume));
    assert_eq!(state.id(), id);
    assert!(state.active());
    assert!(!state.claim_report("song"));
    assert!(!state.claim_scrobble("song"));
    assert!(state.claim_terminal(state.id(), true));
    assert!(!state.claim_terminal(state.id(), true));
}

#[test]
fn replaying_the_same_queue_entry_creates_another_reportable_occurrence() {
    let entry = entry("song");
    let mut state = PlaybackOccurrence::default();
    state.select(entry.id(), "song", Selection::New);
    let first = state.id();
    assert!(state.claim_report("song"));
    assert!(state.claim_scrobble("song"));
    assert!(state.select(entry.id(), "song", Selection::New));
    assert_ne!(state.id(), first);
    assert!(state.claim_report("song"));
    assert!(state.claim_scrobble("song"));
}

#[test]
fn a_duplicate_song_at_another_entry_cannot_resume_the_previous_occurrence() {
    let first = entry("same");
    let second = entry("same");
    let mut state = PlaybackOccurrence::default();
    state.select(first.id(), "same", Selection::New);
    let id = state.id();
    state.claim_report("same");
    assert!(state.select(second.id(), "same", Selection::Resume));
    assert_ne!(state.id(), id);
    assert!(state.claim_report("same"));
}

#[test]
fn queue_reordering_preserves_retry_identity_and_replacing_it_does_not() {
    use crate::models::PlaybackQueue;
    let mut queue = PlaybackQueue::from_songs(
        vec![entry("same").song, entry("same").song],
        QueueOrigin::Manual,
    );
    let mut state = PlaybackOccurrence::default();
    state.select(queue.entry_id(0).unwrap(), "same", Selection::New);
    let id = state.id();
    assert!(queue.move_entry(0, 1));
    assert!(!state.select(queue.entry_id(1).unwrap(), "same", Selection::Resume));
    assert_eq!(state.id(), id);
    queue.remove(1);
    queue.append_entries([entry("same")]);
    assert!(state.select(queue.entry_id(1).unwrap(), "same", Selection::Resume));
    assert_ne!(state.id(), id);
}

#[test]
fn restoration_exposes_identity_without_consuming_reporting_until_play() {
    let entry = entry("song");
    let mut state = PlaybackOccurrence::default();
    state.restore(entry.id(), "song");
    let id = state.id();
    assert!(id.is_some());
    assert!(!state.active());
    assert!(!state.claim_terminal(state.id(), true));
    assert!(!state.claim_report("song"));
    assert!(!state.claim_scrobble("song"));
    assert!(!state.select(entry.id(), "song", Selection::Resume));
    assert_eq!(state.id(), id);
    assert!(state.active());
    assert!(state.claim_report("song"));
    assert!(state.claim_scrobble("song"));
}

#[test]
fn playing_a_completed_track_starts_a_new_occurrence() {
    let entry = entry("song");
    let mut state = PlaybackOccurrence::default();
    state.select(entry.id(), "song", Selection::New);
    let id = state.id();
    assert!(state.claim_terminal(state.id(), true));
    assert!(state.claim_scrobble("song"));
    assert!(state.select(entry.id(), "song", Selection::Resume));
    assert_ne!(state.id(), id);
    assert!(state.claim_scrobble("song"));
}

#[test]
fn stop_discards_old_claims_and_reselecting_the_same_song_starts_fresh() {
    let entry = entry("song");
    let mut state = PlaybackOccurrence::default();
    state.select(entry.id(), "song", Selection::New);
    let id = state.id();
    state.clear();
    assert_eq!(state.id(), None);
    assert!(!state.active());
    assert!(!state.claim_report("song"));
    assert!(!state.claim_scrobble("song"));
    assert!(!state.claim_terminal(state.id(), true));
    assert!(state.select(entry.id(), "song", Selection::Resume));
    assert_ne!(state.id(), id);
}

#[test]
fn reports_for_a_previous_song_do_not_consume_current_reporting() {
    let entry = entry("current");
    let mut state = PlaybackOccurrence::default();
    state.select(entry.id(), "current", Selection::New);
    assert!(!state.claim_report("previous"));
    assert!(!state.claim_scrobble("previous"));
    assert!(state.claim_report("current"));
    assert!(state.claim_scrobble("current"));
}

#[test]
fn repeated_occurrences_reject_previous_media_positions_but_retry_keeps_the_path() {
    use crate::app::desktop::media_command_state::{absolute_seek, SeekAction};
    use crate::playback_timeline::PlaybackTimeline;
    use std::time::Duration;
    let entry = entry("same");
    let mut state = PlaybackOccurrence::default();
    state.select(entry.id(), "same", Selection::New);
    let old = crate::media_session::track_id("https://server", "same", state.id());
    state.select(entry.id(), "same", Selection::Resume);
    assert_eq!(
        crate::media_session::track_id("https://server", "same", state.id()),
        old
    );
    state.select(entry.id(), "same", Selection::New);
    let current = crate::media_session::track_id("https://server", "same", state.id());
    let timeline = Some(PlaybackTimeline {
        position: Duration::ZERO,
        duration: Some(Duration::from_secs(100)),
    });
    assert_eq!(
        absolute_seek(timeline, Some(&current), &old, Duration::from_secs(10)),
        SeekAction::Ignore
    );
    assert_eq!(
        absolute_seek(timeline, Some(&current), &current, Duration::from_secs(10)),
        SeekAction::Seek(Duration::from_secs(10))
    );
}

#[test]
fn late_terminal_event_for_a_repeated_song_cannot_end_the_new_occurrence() {
    let entry = entry("same");
    let mut state = PlaybackOccurrence::default();
    state.select(entry.id(), "same", Selection::New);
    let old = state.id();
    state.select(entry.id(), "same", Selection::New);
    assert!(!state.claim_terminal(old, true));
    assert!(!state.claim_terminal(old, false));
    assert!(state.active());
    assert!(state.claim_terminal(state.id(), true));
}
