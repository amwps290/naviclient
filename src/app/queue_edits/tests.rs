use super::*;
use crate::models::{PlaybackSession, QueueEntry, QueueEntryId, TranscodingQuality};
use std::time::{Duration, Instant};

fn song(id: &str) -> Song {
    Song {
        id: id.into(),
        duration: Some(180),
        ..Default::default()
    }
}

fn state() -> AppState {
    let mut state = AppState {
        queue: PlaybackQueue::from_snapshot(
            vec![song("same"), song("same"), song("other"), song("tail")],
            vec![
                QueueOrigin::Playlist,
                QueueOrigin::Manual,
                QueueOrigin::DailyMix,
                QueueOrigin::SongRadio,
            ],
        ),
        queue_index: Some(1),
        now_playing: Some(song("same")),
        now_playing_quality: Some(TranscodingQuality::Original),
        shuffle_history: ShuffleHistory::restore(vec![0, 1, 0, 1], vec![3, 2, 3]),
        ..Default::default()
    };
    state
        .occurrence
        .restore(state.queue.entry_id(1).unwrap(), "same");
    state.song_selection.toggle(SongScope::Queue, 1, 4);
    state.song_selection.toggle(SongScope::Queue, 3, 4);
    state
}

fn entry_ids(state: &AppState) -> Vec<QueueEntryId> {
    state.queue.entries().iter().map(QueueEntry::id).collect()
}

fn select_current(state: &mut AppState, index: usize) {
    let song = state.queue.song(index).unwrap().clone();
    state.queue_index = Some(index);
    state
        .occurrence
        .restore(state.queue.entry_id(index).unwrap(), &song.id);
    state.now_playing = Some(song);
}

fn history_entries(state: &AppState) -> (Vec<QueueEntryId>, Vec<QueueEntryId>) {
    let ids = |indices: &[usize]| {
        indices
            .iter()
            .map(|index| state.queue.entry_id(*index).expect("valid history index"))
            .collect()
    };
    (
        ids(&state.shuffle_history.played),
        ids(&state.shuffle_history.forward),
    )
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    entries: Vec<QueueEntryId>,
    index: Option<usize>,
    history: (Vec<usize>, Vec<usize>),
    song: Option<String>,
    quality: Option<TranscodingQuality>,
    occurrence: Option<crate::models::PlaybackId>,
    selection: super::super::song_selection::SongSelection,
}

fn snapshot(state: &AppState) -> Snapshot {
    Snapshot {
        entries: entry_ids(state),
        index: state.queue_index,
        history: state.shuffle_history.snapshot(),
        song: state.now_playing.as_ref().map(|song| song.id.clone()),
        quality: state.now_playing_quality,
        occurrence: state.occurrence.id(),
        selection: state.song_selection.clone(),
    }
}

#[test]
fn inserting_into_an_empty_queue_enables_play_without_creating_a_playback_occurrence() {
    use super::super::media_command_state::MediaCapabilities;

    for songs in [vec![song("first")], vec![song("first"), song("second")]] {
        let mut state = AppState::default();
        assert!(state.edit_queue().insert_next(songs));
        assert_eq!(state.queue_index, Some(0));
        assert_eq!(state.queue.song(0).unwrap().id, "first");
        assert!(state.now_playing.is_none());
        assert!(state.now_playing_quality.is_none());
        assert_eq!(state.occurrence.id(), None);
        assert_eq!(state.shuffle_history.snapshot(), (vec![], vec![]));
        assert!(
            MediaCapabilities::for_queue(
                true,
                state.queue_index,
                state.queue.len(),
                state.playback_modes,
                state.shuffle_history.played.len(),
            )
            .play
        );
    }
}

#[test]
fn block_insertion_preserves_order_current_occurrence_and_both_history_branches() {
    let mut state = state();
    let before = snapshot(&state);
    let history = history_entries(&state);
    assert!(state
        .edit_queue()
        .insert_next(vec![song("new-a"), song("new-b")]));
    assert_eq!(
        state.queue.song_ids(),
        ["same", "same", "new-a", "new-b", "other", "tail"]
    );
    assert_eq!(state.queue_index, Some(1));
    assert_eq!(state.queue.entry_id(1), Some(before.entries[1]));
    assert_eq!(state.queue.origin(2), Some(QueueOrigin::Manual));
    assert_eq!(state.queue.origin(3), Some(QueueOrigin::Manual));
    assert_eq!(history_entries(&state), history);
    assert_eq!(state.occurrence.id(), before.occurrence);
    assert_eq!(state.now_playing.as_ref().unwrap().id, before.song.unwrap());
    assert_eq!(state.now_playing_quality, before.quality);
    assert_eq!(state.song_selection.cursor(), Some((SongScope::Queue, 5)));
    assert_eq!(
        state
            .song_selection
            .selected_indices(SongScope::Queue, 6)
            .collect::<Vec<_>>(),
        [1, 5]
    );
    assert_eq!(state.song_selection.multi().unwrap().anchor, 5);
}

#[test]
fn insertion_without_a_selected_track_uses_the_front_and_keeps_existing_history_entries() {
    let mut state = state();
    state.queue_index = None;
    state.now_playing = None;
    state.now_playing_quality = None;
    state.occurrence.clear();
    let history = history_entries(&state);
    let before = entry_ids(&state);
    assert!(state.edit_queue().insert_next(vec![song("first")]));
    assert_eq!(state.queue.song(0).unwrap().id, "first");
    assert_eq!(&entry_ids(&state)[1..], before);
    assert_eq!(state.queue_index, None);
    assert_eq!(history_entries(&state), history);
}

#[test]
fn append_preserves_current_selection_and_origins_and_selects_the_first_entry_when_empty() {
    for mut state in [AppState::default(), state()] {
        let before = snapshot(&state);
        let history = history_entries(&state);
        assert!(state
            .edit_queue()
            .append(vec![song("mix-a"), song("mix-b")], QueueOrigin::DailyMix));
        assert_eq!(&entry_ids(&state)[..before.entries.len()], before.entries);
        assert_eq!(state.queue_index, before.index.or(Some(0)));
        assert_eq!(
            state.queue.origin(state.queue.len() - 1),
            Some(QueueOrigin::DailyMix)
        );
        assert_eq!(history_entries(&state), history);
        assert_eq!(state.occurrence.id(), before.occurrence);
        assert_eq!(
            state.now_playing.as_ref().map(|song| song.id.clone()),
            before.song
        );
    }
}

#[test]
fn empty_insertions_invalid_removals_and_invalid_moves_preserve_all_queue_state() {
    for mut state in [AppState::default(), state()] {
        let before = snapshot(&state);
        assert!(!state.edit_queue().insert_next(vec![]));
        assert!(!state.edit_queue().append(vec![], QueueOrigin::Manual));
        assert_eq!(state.edit_queue().remove(usize::MAX), QueueRemoval::Missing);
        for (from, to) in [(usize::MAX, 0), (0, usize::MAX), (0, 0)] {
            assert!(!state.edit_queue().move_entry(from, to));
        }
        assert_eq!(snapshot(&state), before);
    }
}

#[test]
fn every_move_preserves_the_selected_entry_and_history_occurrences_even_for_duplicate_songs() {
    for current in 0..4 {
        for from in 0..4 {
            for to in 0..4 {
                let mut state = state();
                select_current(&mut state, current);
                let current_entry = state.queue.entry_id(current);
                let history = history_entries(&state);
                let occurrence = state.occurrence.id();
                assert_eq!(state.edit_queue().move_entry(from, to), from != to);
                assert_eq!(
                    state
                        .queue_index
                        .and_then(|index| state.queue.entry_id(index)),
                    current_entry
                );
                assert_eq!(history_entries(&state), history);
                assert_eq!(state.occurrence.id(), occurrence);
            }
        }
    }
}

#[test]
fn removing_an_unselected_entry_preserves_the_selected_identity_and_filters_only_its_history() {
    for current in 0..4 {
        for removed in (0..4).filter(|removed| *removed != current) {
            let mut state = state();
            select_current(&mut state, current);
            let current_entry = state.queue.entry_id(current);
            let removed_entry = state.queue.entry_id(removed).unwrap();
            let (mut played, mut forward) = history_entries(&state);
            played.retain(|entry| *entry != removed_entry);
            forward.retain(|entry| *entry != removed_entry);
            assert_eq!(
                state.edit_queue().remove(removed),
                QueueRemoval::KeptCurrent
            );
            assert_eq!(
                state
                    .queue_index
                    .and_then(|index| state.queue.entry_id(index)),
                current_entry
            );
            assert_eq!(history_entries(&state), (played, forward));
        }
    }
}

#[test]
fn removing_the_selected_entry_returns_its_successor_or_last_survivor_without_claiming_playback() {
    for current in 0..4 {
        let mut state = state();
        select_current(&mut state, current);
        let before = snapshot(&state);
        let expected_entry = before
            .entries
            .get(current + 1)
            .unwrap_or_else(|| &before.entries[current - 1]);
        let effect = state.edit_queue().remove(current);
        let QueueRemoval::PlayReplacement(next) = effect else {
            panic!("expected replacement")
        };
        assert_eq!(state.queue_index, Some(next));
        assert_eq!(state.queue.entry_id(next), Some(*expected_entry));
        assert_eq!(state.occurrence.id(), before.occurrence);
        assert_eq!(state.now_playing.as_ref().unwrap().id, before.song.unwrap());
    }
}

#[test]
fn removing_the_only_selected_entry_requests_stop_and_clears_queue_history() {
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(vec![song("only")], QueueOrigin::Manual),
        queue_index: Some(0),
        shuffle_history: ShuffleHistory::restore(vec![0, 0], vec![0]),
        ..Default::default()
    };
    assert_eq!(state.edit_queue().remove(0), QueueRemoval::Stop);
    assert!(state.queue.is_empty());
    assert_eq!(state.queue_index, None);
    assert_eq!(state.shuffle_history.snapshot(), (vec![], vec![]));
}

#[test]
fn trimming_keeps_current_identity_and_removes_only_the_discarded_history_occurrences() {
    let mut state = state();
    let before = snapshot(&state);
    let (played, mut forward) = history_entries(&state);
    forward.retain(|entry| before.entries[..2].contains(entry));
    assert!(state.edit_queue().trim_after_current());
    assert_eq!(entry_ids(&state), before.entries[..2]);
    assert_eq!(state.queue_index, before.index);
    assert_eq!(history_entries(&state), (played, forward));
    assert_eq!(state.occurrence.id(), before.occurrence);
    assert!(!state.edit_queue().trim_after_current());
}

#[test]
fn clearing_discards_queue_selection_and_history_and_allows_a_fresh_queued_start() {
    let mut state = state();
    state.edit_queue().clear();
    assert!(state.queue.is_empty());
    assert_eq!(state.queue_index, None);
    assert_eq!(state.shuffle_history.snapshot(), (vec![], vec![]));
    assert!(state.edit_queue().insert_next(vec![song("new")]));
    assert_eq!(state.queue_index, Some(0));
    assert_eq!(state.queue.song_ids(), ["new"]);
    assert_eq!(state.shuffle_history.snapshot(), (vec![], vec![]));
}

#[test]
fn queue_edits_preserve_a_restored_position_and_save_remapped_origins_and_history() {
    use super::super::session_state::SessionState;

    let mut state = state();
    let mut session = SessionState::new(Instant::now());
    session.bind(Some("fixture-server"), Some("http://fixture.test"));
    let (queue, queue_origins) = state.queue.snapshot();
    let (shuffle_played, shuffle_forward) = state.shuffle_history.snapshot();
    assert!(session.restore(
        &mut state,
        PlaybackSession {
            server_url: "http://fixture.test".into(),
            queue,
            queue_origins,
            queue_index: Some(1),
            position_secs: 42.125,
            shuffle_played,
            shuffle_forward,
            ..Default::default()
        }
    ));
    let occurrence = state.occurrence.id();
    let current = state.queue.entry_id(1);
    assert!(state.edit_queue().insert_next(vec![song("next")]));
    assert!(state.edit_queue().move_entry(1, 0));
    assert_eq!(state.edit_queue().remove(3), QueueRemoval::KeptCurrent);
    assert_eq!(state.queue.entry_id(0), current);
    assert_eq!(state.occurrence.id(), occurrence);
    assert!(!state.occurrence.active());
    assert_eq!(
        session.position(Duration::ZERO),
        Duration::from_millis(42_125)
    );
    let saved = session
        .snapshot(&state, &crate::audio::PlaybackState::default())
        .unwrap();
    assert_eq!(saved.queue_index, Some(0));
    assert_eq!(saved.position_secs, 42.125);
    assert_eq!(
        saved.queue_origins,
        [
            QueueOrigin::Manual,
            QueueOrigin::Playlist,
            QueueOrigin::Manual,
            QueueOrigin::SongRadio
        ]
    );
    assert_eq!(saved.shuffle_played, [1, 0, 1, 0]);
    assert_eq!(saved.shuffle_forward, [3, 3]);
    let saved = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    let mut restored = AppState::default();
    assert!(session.restore(&mut restored, saved));
    assert_eq!(restored.queue_index, Some(0));
    assert_eq!(restored.queue.song(0).unwrap().id, "same");
    assert_eq!(restored.queue.origin(0), Some(QueueOrigin::Manual));
    assert_eq!(
        session.position(Duration::ZERO),
        Duration::from_millis(42_125)
    );
}

fn owned_songs(count: usize) -> Vec<Song> {
    (0..count)
        .map(|index| Song {
            id: "same".into(),
            title: format!("Owned occurrence {index}"),
            album: "Album".into(),
            album_id: Some("album-id".into()),
            artist: "Artist".into(),
            artist_id: Some("artist-id".into()),
            genre: Some("Genre".into()),
            cover_art: Some("cover-id".into()),
            suffix: Some("flac".into()),
            content_type: Some("audio/flac".into()),
            path: Some(format!("/music/{index}.flac")),
            track: Some(index as i32),
            disc: Some(2),
            bit_rate: Some(900),
            size: Some(12_345_678),
            replay_gain_track_gain: Some(-4.5),
            ..song("same")
        })
        .collect()
}

/// Compare live allocations, never dereference saved pointers. A deep copy
/// cannot reuse the source allocation while that source is still alive.
fn song_allocations(song: &Song) -> Vec<*const u8> {
    [
        Some(&song.id),
        Some(&song.title),
        Some(&song.album),
        Some(&song.artist),
        song.album_id.as_ref(),
        song.artist_id.as_ref(),
        song.genre.as_ref(),
        song.cover_art.as_ref(),
        song.suffix.as_ref(),
        song.content_type.as_ref(),
        song.path.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.as_ptr())
    .collect()
}

fn selected_entries(
    state: &AppState,
) -> (
    Option<QueueEntryId>,
    Option<QueueEntryId>,
    Vec<QueueEntryId>,
) {
    let selection = &state.song_selection;
    (
        selection
            .cursor()
            .and_then(|(_, index)| state.queue.entry_id(index)),
        selection
            .multi()
            .and_then(|multi| state.queue.entry_id(multi.anchor)),
        selection
            .selected_indices(SongScope::Queue, state.queue.len())
            .map(|index| state.queue.entry_id(index).unwrap())
            .collect(),
    )
}

#[test]
fn owned_insertions_move_all_metadata_and_preserve_old_selection_and_history_identities() {
    for initial_len in [0, 4] {
        for current in [None, Some(0), Some(1), Some(3), Some(usize::MAX)] {
            for count in [0, 1, 3, 32] {
                let mut state = if initial_len == 0 {
                    AppState::default()
                } else {
                    state()
                };
                state.queue_index = current;
                let before = snapshot(&state);
                let history = history_entries(&state);
                let selection = selected_entries(&state);
                let songs = owned_songs(count);
                let payload = serde_json::to_value(&songs).unwrap();
                let allocations: Vec<_> = songs.iter().map(song_allocations).collect();
                let insert_at = match current {
                    None => 0,
                    Some(usize::MAX) => initial_len,
                    Some(index) => (index + 1).min(initial_len),
                };

                assert_eq!(state.edit_queue().insert_next(songs), count > 0);
                if count == 0 {
                    assert_eq!(snapshot(&state), before);
                    continue;
                }
                let inserted = &state.queue.entries()[insert_at..insert_at + count];
                assert_eq!(
                    serde_json::to_value(
                        inserted.iter().map(|entry| &entry.song).collect::<Vec<_>>()
                    )
                    .unwrap(),
                    payload
                );
                for (entry, allocation) in inserted.iter().zip(&allocations) {
                    assert_eq!(&song_allocations(&entry.song), allocation);
                    assert_eq!(entry.origin, QueueOrigin::Manual);
                    assert!(!before.entries.contains(&entry.id()));
                }
                let ids = entry_ids(&state);
                assert_eq!(&ids[..insert_at], &before.entries[..insert_at]);
                assert_eq!(&ids[insert_at + count..], &before.entries[insert_at..]);
                for (index, id) in ids.iter().enumerate() {
                    assert!(!ids[..index].contains(id));
                }
                assert_eq!(
                    state.queue_index,
                    if initial_len == 0 { Some(0) } else { current }
                );
                assert_eq!(history_entries(&state), history);
                assert_eq!(selected_entries(&state), selection);
                assert_eq!(state.occurrence.id(), before.occurrence);
                assert_eq!(
                    state.now_playing.as_ref().map(|song| song.id.clone()),
                    before.song
                );
                assert_eq!(state.now_playing_quality, before.quality);
            }
        }
    }
}

#[test]
fn owned_appends_move_all_metadata_for_every_origin_without_changing_existing_entries() {
    for origin in [
        QueueOrigin::Manual,
        QueueOrigin::Playlist,
        QueueOrigin::DailyMix,
        QueueOrigin::SongRadio,
        QueueOrigin::ArtistRadio,
        QueueOrigin::AutoContinue,
    ] {
        for initial_len in [0, 4] {
            for count in [0, 1, 3, 32] {
                let mut state = if initial_len == 0 {
                    AppState::default()
                } else {
                    state()
                };
                let before = snapshot(&state);
                let history = history_entries(&state);
                let songs = owned_songs(count);
                let payload = serde_json::to_value(&songs).unwrap();
                let allocations: Vec<_> = songs.iter().map(song_allocations).collect();

                assert_eq!(state.edit_queue().append(songs, origin), count > 0);
                if count == 0 {
                    assert_eq!(snapshot(&state), before);
                    continue;
                }
                let appended = &state.queue.entries()[initial_len..];
                assert_eq!(
                    serde_json::to_value(
                        appended.iter().map(|entry| &entry.song).collect::<Vec<_>>()
                    )
                    .unwrap(),
                    payload
                );
                for (entry, allocation) in appended.iter().zip(&allocations) {
                    assert_eq!(&song_allocations(&entry.song), allocation);
                    assert_eq!(entry.origin, origin);
                    assert!(!before.entries.contains(&entry.id()));
                }
                let ids = entry_ids(&state);
                assert_eq!(&ids[..initial_len], &before.entries);
                for (index, id) in ids.iter().enumerate() {
                    assert!(!ids[..index].contains(id));
                }
                assert_eq!(state.queue_index, before.index.or(Some(0)));
                assert_eq!(history_entries(&state), history);
                assert_eq!(state.song_selection, before.selection);
                assert_eq!(state.occurrence.id(), before.occurrence);
                assert_eq!(
                    state.now_playing.as_ref().map(|song| song.id.clone()),
                    before.song
                );
                assert_eq!(state.now_playing_quality, before.quality);
            }
        }
    }
}

#[test]
fn prepared_entry_appends_keep_their_mixed_origins_identities_and_allocations() {
    let mut state = state();
    let before = snapshot(&state);
    let entries: Vec<_> = owned_songs(3)
        .into_iter()
        .zip([
            QueueOrigin::SongRadio,
            QueueOrigin::AutoContinue,
            QueueOrigin::DailyMix,
        ])
        .map(|(song, origin)| QueueEntry::new(song, origin))
        .collect();
    let identities: Vec<_> = entries.iter().map(QueueEntry::id).collect();
    let origins: Vec<_> = entries.iter().map(|entry| entry.origin).collect();
    let allocations: Vec<_> = entries
        .iter()
        .map(|entry| song_allocations(&entry.song))
        .collect();

    assert!(state
        .edit_queue()
        .append_entries(entries.into_iter().filter(|_| true)));

    let appended = &state.queue.entries()[before.entries.len()..];
    assert_eq!(
        appended.iter().map(QueueEntry::id).collect::<Vec<_>>(),
        identities
    );
    assert_eq!(
        appended
            .iter()
            .map(|entry| entry.origin)
            .collect::<Vec<_>>(),
        origins
    );
    assert_eq!(
        appended
            .iter()
            .map(|entry| song_allocations(&entry.song))
            .collect::<Vec<_>>(),
        allocations
    );
    assert_eq!(&entry_ids(&state)[..before.entries.len()], &before.entries);
    assert_eq!(state.song_selection, before.selection);
    assert_eq!(state.shuffle_history.snapshot(), before.history);
    assert_eq!(state.queue_index, before.index);
    assert_eq!(state.occurrence.id(), before.occurrence);
}

#[test]
fn copying_a_browsed_collection_once_preserves_the_source_and_moves_the_copy_into_the_queue() {
    for insert_next in [false, true] {
        let source = owned_songs(3);
        let source_payload = serde_json::to_value(&source).unwrap();
        let source_allocations: Vec<_> = source.iter().map(song_allocations).collect();
        let transfer = source.clone();
        let transfer_allocations: Vec<_> = transfer.iter().map(song_allocations).collect();
        for (source, transfer) in source_allocations.iter().zip(&transfer_allocations) {
            for (source, transfer) in source.iter().zip(transfer) {
                assert_ne!(source, transfer);
            }
        }
        let mut state = AppState::default();
        assert!(if insert_next {
            state.edit_queue().insert_next(transfer)
        } else {
            state.edit_queue().append(transfer, QueueOrigin::Playlist)
        });

        assert_eq!(serde_json::to_value(&source).unwrap(), source_payload);
        assert_eq!(
            state
                .queue
                .songs()
                .map(song_allocations)
                .collect::<Vec<_>>(),
            transfer_allocations
        );
        assert_eq!(
            source.iter().map(song_allocations).collect::<Vec<_>>(),
            source_allocations
        );
    }
}
