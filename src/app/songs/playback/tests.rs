use super::*;
use crate::models::{ListeningHistory, PlaybackQueue, QueueEntry};

fn songs() -> Vec<Song> {
    ["First", "Second", "Third"]
        .map(|title| Song {
            id: "duplicate".into(),
            title: title.into(),
            album: format!("{title} album"),
            ..Default::default()
        })
        .to_vec()
}

fn queue(songs: &[Song]) -> PlaybackQueue {
    PlaybackQueue::from_snapshot(
        songs.to_vec(),
        vec![
            QueueOrigin::DailyMix,
            QueueOrigin::Playlist,
            QueueOrigin::SongRadio,
        ],
    )
}

fn projection<'a>(scope: SongScope, songs: &'a [Song], queue: &'a [QueueEntry]) -> SongList<'a> {
    if scope == SongScope::Queue {
        SongList::Queue(queue)
    } else {
        SongList::Songs(songs)
    }
}

fn assert_replacement(
    playback: SongPlayback,
    expected: &[Song],
    expected_index: usize,
    expected_origin: QueueOrigin,
) {
    let SongPlayback::Replace {
        songs,
        index,
        origin,
    } = playback
    else {
        panic!("expected a queue replacement")
    };
    assert_eq!(index, expected_index);
    assert_eq!(origin, expected_origin);
    assert_eq!(
        serde_json::to_value(songs).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn row_requests_preserve_each_scopes_queue_extent_start_index_and_origin() {
    let songs = songs();
    let queue = queue(&songs);
    for scope in SongScope::ALL {
        let list = projection(scope, &songs, queue.entries());
        for index in 0..songs.len() {
            let playback = SongPlayback::row(scope, list, index).unwrap();
            if scope == SongScope::Queue {
                assert!(matches!(playback, SongPlayback::QueueIndex(target) if target == index));
            } else if scope == SongScope::History {
                assert_replacement(playback, &songs[index..=index], 0, QueueOrigin::Manual);
            } else {
                assert_replacement(
                    playback,
                    &songs,
                    index,
                    if scope == SongScope::PlaylistDetail {
                        QueueOrigin::Playlist
                    } else {
                        QueueOrigin::Manual
                    },
                );
            }
        }
        for index in [songs.len(), usize::MAX] {
            assert!(SongPlayback::row(scope, list, index).is_none());
        }
    }
}

#[test]
fn selection_requests_keep_row_order_duplicates_and_filter_every_shrinking_list_boundary() {
    let songs = songs();
    let queue = queue(&songs);
    for scope in SongScope::ALL {
        for mask in 0..8 {
            let mut selection = SongSelection::default();
            for index in (0..3).rev() {
                if mask & (1 << index) != 0 {
                    selection.toggle(scope, index, 3);
                }
            }
            for len in 0..=3 {
                let list = projection(scope, &songs[..len], &queue.entries()[..len]);
                let playback = SongPlayback::selected(scope, list, &selection);
                let selected: Vec<_> = (0..len).filter(|index| mask & (1 << index) != 0).collect();
                if selected.is_empty() {
                    assert!(playback.is_none());
                } else if scope == SongScope::Queue {
                    assert!(
                        matches!(playback, Some(SongPlayback::QueueIndex(index)) if index == selected[0])
                    );
                } else {
                    let expected: Vec<_> = selected
                        .into_iter()
                        .map(|index| songs[index].clone())
                        .collect();
                    assert_replacement(
                        playback.unwrap(),
                        &expected,
                        0,
                        if scope == SongScope::PlaylistDetail {
                            QueueOrigin::Playlist
                        } else {
                            QueueOrigin::Manual
                        },
                    );
                }
            }
        }
    }
}

#[test]
fn history_row_replays_one_event_while_multi_playback_keeps_the_selected_event_order() {
    let songs = songs();
    let mut history = ListeningHistory::default();
    for (index, song) in songs.iter().enumerate() {
        history.record_started(song.clone(), index as u64);
    }
    let mut state = AppState::default();
    state.listening.restore(None, history);
    let recent: Vec<_> = songs.into_iter().rev().collect();
    assert_replacement(
        state.song_playback_at(SongScope::History, 1).unwrap(),
        &recent[1..=1],
        0,
        QueueOrigin::Manual,
    );
    state.song_selection.toggle(SongScope::History, 2, 3);
    state.song_selection.toggle(SongScope::History, 0, 3);
    assert_replacement(
        state.selected_playback(SongScope::History).unwrap(),
        &[recent[0].clone(), recent[2].clone()],
        0,
        QueueOrigin::Manual,
    );
    assert_eq!(
        serde_json::to_value(state.listening.recent_songs()).unwrap(),
        serde_json::to_value(recent).unwrap()
    );
}

#[test]
fn preparing_queue_playback_keeps_occurrences_origins_current_song_and_selection() {
    let songs = songs();
    let mut state = AppState {
        queue: queue(&songs),
        queue_index: Some(2),
        now_playing: Some(songs[2].clone()),
        ..Default::default()
    };
    state.song_selection.toggle(SongScope::Queue, 2, 3);
    state.song_selection.toggle(SongScope::Queue, 0, 3);
    let identities: Vec<_> = state.queue.entries().iter().map(QueueEntry::id).collect();
    let origins: Vec<_> = state
        .queue
        .entries()
        .iter()
        .map(|entry| entry.origin)
        .collect();
    let selection = state.song_selection.clone();
    for index in 0..3 {
        assert!(
            matches!(state.song_playback_at(SongScope::Queue, index), Some(SongPlayback::QueueIndex(target)) if target == index)
        );
    }
    assert!(matches!(
        state.selected_playback(SongScope::Queue),
        Some(SongPlayback::QueueIndex(0))
    ));
    assert_eq!(
        state
            .queue
            .entries()
            .iter()
            .map(QueueEntry::id)
            .collect::<Vec<_>>(),
        identities
    );
    assert_eq!(
        state
            .queue
            .entries()
            .iter()
            .map(|entry| entry.origin)
            .collect::<Vec<_>>(),
        origins
    );
    assert_eq!(state.song_selection, selection);
    assert_eq!(state.queue_index, Some(2));
    assert_eq!(state.now_playing.unwrap().title, "Third");
}

#[test]
fn empty_missing_or_other_scope_selection_never_prepares_playback() {
    let songs = songs();
    let queue = queue(&songs);
    for scope in SongScope::ALL {
        assert!(SongPlayback::row(scope, projection(scope, &[], &[]), 0).is_none());
        for other in SongScope::ALL {
            if other == scope {
                continue;
            }
            let mut selection = SongSelection::default();
            selection.toggle(other, 0, 3);
            assert!(SongPlayback::selected(
                scope,
                projection(scope, &songs, queue.entries()),
                &selection
            )
            .is_none());
        }
    }
    let state = AppState::default();
    for scope in SongScope::ALL {
        assert!(state.song_playback_at(scope, 0).is_none());
        assert!(state.selected_playback(scope).is_none());
    }
}

#[test]
fn replacement_requests_own_their_snapshot_after_the_source_changes() {
    let mut songs = songs();
    let original = songs.clone();
    let row = SongPlayback::row(SongScope::PlaylistDetail, SongList::Songs(&songs), 1).unwrap();
    let mut selection = SongSelection::default();
    selection.select_all(SongScope::PlaylistDetail, 3);
    let selected = SongPlayback::selected(
        SongScope::PlaylistDetail,
        SongList::Songs(&songs),
        &selection,
    )
    .unwrap();
    songs[0].title = "Changed title".into();
    songs.clear();
    assert_replacement(row, &original, 1, QueueOrigin::Playlist);
    assert_replacement(selected, &original, 0, QueueOrigin::Playlist);
}
