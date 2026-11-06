use super::*;
use crate::audio::test_transport::Fixture;
use crate::audio::PlaybackState;
use crate::models::{PlayModes, QueueOrigin, RepeatMode, Song};

fn queue() -> PlaybackQueue {
    PlaybackQueue::from_songs(
        ["same", "other", "same"]
            .map(|id| Song {
                id: id.into(),
                ..Default::default()
            })
            .to_vec(),
        QueueOrigin::Manual,
    )
}

fn reserve(state: &mut PreloadState, queue: &PlaybackQueue, index: usize) -> u64 {
    assert!(state.reserve(queue, index, TranscodingQuality::Original, |_| true));
    state.pending.unwrap().id
}

#[test]
fn reuse_requires_the_same_occurrence_quality_and_zero_start_position() {
    let queue = queue();
    let mut state = PreloadState::default();
    let id = reserve(&mut state, &queue, 2);
    let entry = queue.entry_id(2).unwrap();
    assert_eq!(state.reserved_index(&queue), Some(2));
    assert_eq!(
        state.reusable_id(entry, TranscodingQuality::Original, Duration::ZERO),
        Some(id)
    );
    assert_eq!(
        state.reusable_id(
            queue.entry_id(0).unwrap(),
            TranscodingQuality::Original,
            Duration::ZERO
        ),
        None
    );
    assert_eq!(
        state.reusable_id(entry, TranscodingQuality::Kbps320, Duration::ZERO),
        None
    );
    assert_eq!(
        state.reusable_id(entry, TranscodingQuality::Original, Duration::from_nanos(1)),
        None
    );
    // A failed foreground Play may reuse the same preparation on its next try.
    assert!(state.reserved());
    assert_eq!(state.reserved_index(&queue), Some(2));
}

#[test]
fn old_events_cannot_consume_new_reservations_and_matching_events_are_used_once() {
    let queue = queue();
    let mut state = PreloadState::default();
    let old = reserve(&mut state, &queue, 0);
    state.clear();
    let new = reserve(&mut state, &queue, 2);
    assert!(new > old);
    assert_eq!(
        state.take_transition(old, &queue),
        Err(PreloadTransitionError::Unreserved)
    );
    assert_eq!(state.reserved_index(&queue), Some(2));
    let started = state.take_transition(new, &queue).unwrap();
    assert_eq!(started.entry, queue.entry_id(2).unwrap());
    assert_eq!(started.index, 2);
    assert!(!state.reserved());
    assert_eq!(
        state.take_transition(new, &queue),
        Err(PreloadTransitionError::Unreserved)
    );
}

#[test]
fn queue_changes_cannot_promote_another_copy_of_the_same_song() {
    for change in 0..3 {
        let mut queue = queue();
        let mut state = PreloadState::default();
        let id = reserve(&mut state, &queue, 0);
        match change {
            0 => {
                assert!(queue.move_entry(0, 2));
            }
            1 => {
                queue.remove(0).unwrap();
            }
            2 => {
                let (songs, origins) = queue.snapshot();
                queue = PlaybackQueue::from_snapshot(songs, origins);
            }
            _ => unreachable!(),
        }
        assert_eq!(state.reserved_index(&queue), None);
        assert_eq!(
            state.take_transition(id, &queue),
            Err(PreloadTransitionError::QueueChanged)
        );
        assert!(!state.reserved());
        assert!(reserve(&mut state, &queue, 0) > id);
    }
}

#[test]
fn shuffle_uses_a_valid_reservation_and_falls_back_after_its_entry_changes() {
    let mut queue = queue();
    let mut state = PreloadState::default();
    reserve(&mut state, &queue, 2);
    let modes = PlayModes {
        shuffle: true,
        repeat: RepeatMode::Off,
    };
    assert_eq!(
        crate::app::queue_logic::next_queue_index(
            modes,
            0,
            queue.len(),
            true,
            None,
            state.reserved_index(&queue),
            || panic!("the reserved shuffle choice should be reused")
        ),
        Some(2)
    );
    assert!(queue.move_entry(2, 1));
    assert_eq!(
        crate::app::queue_logic::next_queue_index(
            modes,
            0,
            queue.len(),
            true,
            None,
            state.reserved_index(&queue),
            || 1
        ),
        Some(1)
    );
}

#[test]
fn invalid_or_duplicate_reservations_do_not_send_audio_commands() {
    let queue = queue();
    let mut state = PreloadState::default();
    for index in [queue.len(), usize::MAX] {
        assert!(
            !state.reserve(&queue, index, TranscodingQuality::Original, |_| {
                panic!("an invalid queue entry must not be submitted")
            })
        );
    }
    assert!(!state.reserved());
    let id = reserve(&mut state, &queue, 2);
    assert!(
        !state.reserve(&queue, 0, TranscodingQuality::Original, |_| {
            panic!("an accepted reservation must not be replaced implicitly")
        })
    );
    assert_eq!(state.pending.unwrap().id, id);
    assert_eq!(state.reserved_index(&queue), Some(2));
}

#[test]
fn rejected_audio_commands_leave_no_reservation_and_can_be_retried() {
    let queue = queue();
    let mut state = PreloadState::default();
    let mut disconnected = Fixture::new(PlaybackState::default());
    disconnected.disconnect();
    let submit = |audio: &crate::audio::AudioHandle, id| {
        audio.preload(id, "fixture".into(), "song".into(), None, 1.0)
    };
    assert!(
        !state.reserve(&queue, 2, TranscodingQuality::Original, |id| {
            submit(&disconnected.audio, id)
        })
    );
    let rejected_id = state.serial;
    assert!(!state.reserved());
    assert_eq!(state.reserved_index(&queue), None);
    let connected = Fixture::new(PlaybackState::default());
    assert!(
        state.reserve(&queue, 2, TranscodingQuality::Original, |id| {
            submit(&connected.audio, id)
        })
    );
    assert!(state.pending.unwrap().id > rejected_id);
    assert_eq!(state.reserved_index(&queue), Some(2));
}

#[test]
fn exhausted_reservation_sequence_never_reuses_an_old_transition_id() {
    let queue = queue();
    let mut state = PreloadState {
        serial: u64::MAX - 1,
        pending: None,
    };
    assert_eq!(reserve(&mut state, &queue, 2), u64::MAX);
    state.clear();
    assert!(
        !state.reserve(&queue, 0, TranscodingQuality::Original, |_| {
            panic!("an exhausted sequence must not submit a reused ID")
        })
    );
    assert_eq!(state.serial, u64::MAX);
    assert!(!state.reserved());
}
