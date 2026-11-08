use super::*;
use crate::source::ProviderKind;

fn source(profile: &str) -> SourceId {
    SourceId::new(ProviderKind::Bilibili, profile)
}

fn song(id: &str) -> Song {
    Song {
        id: id.into(),
        ..Default::default()
    }
}

#[test]
fn inactive_additions_preserve_order_origins_and_resume_position() {
    let session = PlaybackSession {
        source: Some(source("account")),
        queue: vec![song("old"), song("same"), song("same")],
        queue_origins: vec![QueueOrigin::Playlist; 3],
        queue_index: Some(1),
        position_secs: 42.5,
        shuffle_played: vec![0, 1, 2],
        shuffle_forward: vec![2],
        ..Default::default()
    };
    let result = edit_stored_queue(
        session,
        &source("account"),
        vec![song("new-1"), song("new-2")],
        SourceQueueEdit::InsertNext,
    )
    .unwrap();
    assert_eq!(
        result
            .queue
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["old", "same", "new-1", "new-2", "same"]
    );
    assert_eq!(result.queue_index, Some(1));
    assert_eq!(result.position_secs, 42.5);
    assert_eq!(result.shuffle_played, [0, 1, 4]);
    assert_eq!(result.shuffle_forward, [4]);
    assert_eq!(
        result.queue_origins,
        [
            QueueOrigin::Playlist,
            QueueOrigin::Playlist,
            QueueOrigin::Manual,
            QueueOrigin::Manual,
            QueueOrigin::Playlist
        ]
    );
    assert!(result
        .queue
        .iter()
        .all(|song| song.source == Some(source("account"))));
}

#[test]
fn repeated_edits_append_to_the_inactive_queue_and_select_a_valid_start() {
    let first = edit_stored_queue(
        PlaybackSession::default(),
        &source("account"),
        vec![song("a"), song("a")],
        SourceQueueEdit::Append(QueueOrigin::Playlist),
    )
    .unwrap();
    let result = edit_stored_queue(
        first,
        &source("account"),
        vec![song("b")],
        SourceQueueEdit::Append(QueueOrigin::Manual),
    )
    .unwrap();
    assert_eq!(
        result
            .queue
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "a", "b"]
    );
    assert_eq!(result.queue_index, Some(0));
    assert_eq!(
        result.queue_origins,
        [
            QueueOrigin::Playlist,
            QueueOrigin::Playlist,
            QueueOrigin::Manual
        ]
    );
}

#[test]
fn foreign_items_in_either_stored_or_new_queue_are_rejected() {
    let foreign = Song {
        source: Some(source("other-account")),
        ..song("a")
    };
    assert!(edit_stored_queue(
        PlaybackSession::default(),
        &source("account"),
        vec![song("local"), foreign.clone()],
        SourceQueueEdit::InsertNext
    )
    .is_none());
    assert!(edit_stored_queue(
        PlaybackSession {
            queue: vec![foreign],
            ..Default::default()
        },
        &source("account"),
        vec![song("local")],
        SourceQueueEdit::Append(QueueOrigin::Manual)
    )
    .is_none());
}

#[test]
fn damaged_stored_indices_do_not_overflow_or_resume_a_new_track_at_an_old_position() {
    let result = edit_stored_queue(
        PlaybackSession {
            queue: vec![song("old")],
            queue_index: Some(usize::MAX),
            position_secs: 42.0,
            shuffle_played: vec![usize::MAX, 0],
            shuffle_forward: vec![usize::MAX],
            ..Default::default()
        },
        &source("account"),
        vec![song("new")],
        SourceQueueEdit::InsertNext,
    )
    .unwrap();
    assert_eq!(result.queue_index, Some(0));
    assert_eq!(result.position_secs, 0.0);
    assert_eq!(result.shuffle_played, [1]);
    assert!(result.shuffle_forward.is_empty());
}
