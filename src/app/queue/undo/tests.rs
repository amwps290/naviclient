use super::*;
use crate::models::{QueueEntry, QueueOrigin, Song};
use crate::source::ProviderKind;

fn sample_queue() -> PlaybackQueue {
    PlaybackQueue::from_snapshot(
        ["a", "b", "c", "d"]
            .into_iter()
            .map(|id| Song {
                id: id.into(),
                ..Default::default()
            })
            .collect(),
        vec![
            QueueOrigin::Manual,
            QueueOrigin::DailyMix,
            QueueOrigin::Playlist,
            QueueOrigin::SongRadio,
        ],
    )
}

#[test]
fn undo_preserves_entry_identities_and_expires_after_ttl() {
    let source = SourceId::new(ProviderKind::OpenSubsonic, "srv-1");
    let queue = sample_queue();
    let expected_ids: Vec<_> = queue.entries().iter().map(QueueEntry::id).collect();
    let history = ShuffleHistory::restore(vec![0, 2], vec![3]);
    let start = Instant::now();

    let mut undo = QueueUndoState::default();
    undo.record(QueueUndoRecord {
        source: Some(source.clone()),
        queue: &queue,
        queue_index: Some(1),
        shuffle_history: &history,
        kind: QueueUndoKind::ClearedAll,
        displaced_current: true,
        resume_position: Some(Duration::from_secs(42)),
        now: start,
    });

    assert!(undo.active(start + Duration::from_secs(7)).is_some());
    assert!(!undo.prune(start + Duration::from_secs(7)));

    let taken = undo
        .take(start + Duration::from_secs(2), Some(&source))
        .expect("undo should be available before expiry");
    let restored_ids: Vec<_> = taken.queue.entries().iter().map(QueueEntry::id).collect();
    assert_eq!(restored_ids, expected_ids);
    assert_eq!(taken.queue_index, Some(1));
    assert_eq!(taken.shuffle_history.snapshot(), (vec![0, 2], vec![3]));
    assert!(taken.displaced_current);
    assert_eq!(taken.resume_position, Some(Duration::from_secs(42)));
    assert!(undo.active(start + Duration::from_secs(2)).is_none());
}

#[test]
fn undo_rejects_expired_or_other_source_snapshots() {
    let source_a = SourceId::new(ProviderKind::OpenSubsonic, "a");
    let source_b = SourceId::new(ProviderKind::OpenSubsonic, "b");
    let queue = sample_queue();
    let history = ShuffleHistory::default();
    let start = Instant::now();

    let mut undo = QueueUndoState::default();
    undo.record(QueueUndoRecord {
        source: Some(source_a.clone()),
        queue: &queue,
        queue_index: Some(0),
        shuffle_history: &history,
        kind: QueueUndoKind::Removed(1),
        displaced_current: false,
        resume_position: None,
        now: start,
    });
    assert!(undo
        .take(start + Duration::from_secs(1), Some(&source_b))
        .is_none());

    undo.record(QueueUndoRecord {
        source: Some(source_a.clone()),
        queue: &queue,
        queue_index: Some(0),
        shuffle_history: &history,
        kind: QueueUndoKind::Removed(1),
        displaced_current: false,
        resume_position: None,
        now: start,
    });
    assert!(undo.prune(start + QUEUE_UNDO_TTL));
    assert!(undo.active(start + QUEUE_UNDO_TTL).is_none());
    assert!(undo.take(start + QUEUE_UNDO_TTL, Some(&source_a)).is_none());
}

#[test]
fn move_to_next_target_handles_before_after_and_no_current() {
    // Queue: [0, 1, 2, 3], current = 1 -> next slot is right after index 1.
    assert_eq!(queue_move_to_next_target(0, Some(1), 4), Some(1));
    assert_eq!(queue_move_to_next_target(1, Some(1), 4), None);
    assert_eq!(queue_move_to_next_target(2, Some(1), 4), None);
    assert_eq!(queue_move_to_next_target(3, Some(1), 4), Some(2));

    // When current is the last track (index 3), moving index 0 puts it after index 3 (now index 2 -> target 3).
    assert_eq!(queue_move_to_next_target(0, Some(3), 4), Some(3));
    assert_eq!(queue_move_to_next_target(2, Some(3), 4), Some(3));
    assert_eq!(queue_move_to_next_target(3, Some(3), 4), None);

    // Without a current track, move to front.
    assert_eq!(queue_move_to_next_target(0, None, 4), None);
    assert_eq!(queue_move_to_next_target(2, None, 4), Some(0));

    // Out of bounds / single item.
    assert_eq!(queue_move_to_next_target(0, Some(0), 1), None);
    assert_eq!(queue_move_to_next_target(4, Some(0), 4), None);
}
