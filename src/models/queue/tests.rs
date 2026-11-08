use super::*;
use crate::models::RepeatMode;

fn song(id: &str, title: &str) -> Song {
    Song {
        id: id.into(),
        title: title.into(),
        ..Song::default()
    }
}

fn occurrences(queue: &PlaybackQueue) -> Vec<(&str, QueueOrigin)> {
    queue
        .entries()
        .iter()
        .map(|entry| (entry.song.title.as_str(), entry.origin))
        .collect()
}

#[test]
fn queue_mutations_keep_each_song_with_its_origin() {
    let mut queue =
        PlaybackQueue::from_songs(vec![song("a", "A"), song("b", "B")], QueueOrigin::Playlist);
    queue.insert(1, vec![song("x", "X"), song("y", "Y")], QueueOrigin::Manual);
    queue.append_entries([QueueEntry::new(
        song("radio", "Radio"),
        QueueOrigin::SongRadio,
    )]);
    assert_eq!(queue.song_ids(), ["a", "x", "y", "b", "radio"]);
    assert!(queue.move_entry(4, 1));
    assert_eq!(queue.song(1).unwrap().id, "radio");
    assert_eq!(queue.origin(1), Some(QueueOrigin::SongRadio));
    let removed = queue.remove(2).unwrap();
    assert_eq!(
        (removed.song.id.as_str(), removed.origin),
        ("x", QueueOrigin::Manual)
    );
    queue.truncate(2);
    assert_eq!(
        occurrences(&queue),
        [
            ("A", QueueOrigin::Playlist),
            ("Radio", QueueOrigin::SongRadio)
        ]
    );
    queue.clear();
    assert!(queue.is_empty());
    assert!(queue.snapshot().0.is_empty());
    assert!(queue.snapshot().1.is_empty());
}

#[test]
fn every_move_preserves_duplicate_occurrences_and_their_sources() {
    for from in 0..4 {
        for to in 0..4 {
            let mut queue = PlaybackQueue::from_snapshot(
                vec![
                    song("same", "First"),
                    song("b", "B"),
                    song("same", "Second"),
                    song("c", "C"),
                ],
                vec![
                    QueueOrigin::Manual,
                    QueueOrigin::Playlist,
                    QueueOrigin::DailyMix,
                    QueueOrigin::AutoContinue,
                ],
            );
            let original = [
                ("First", QueueOrigin::Manual),
                ("B", QueueOrigin::Playlist),
                ("Second", QueueOrigin::DailyMix),
                ("C", QueueOrigin::AutoContinue),
            ];
            let selected = original[from];
            let others: Vec<_> = original
                .iter()
                .enumerate()
                .filter_map(|(i, entry)| (i != from).then_some(*entry))
                .collect();
            let moved = queue.move_entry(from, to);
            assert_eq!(moved, from != to);
            let reordered = occurrences(&queue);
            assert_eq!(reordered[to], selected);
            assert_eq!(
                reordered
                    .iter()
                    .enumerate()
                    .filter_map(|(i, entry)| (i != to).then_some(*entry))
                    .collect::<Vec<_>>(),
                others
            );
            let removed = queue.remove(to).unwrap();
            assert_eq!((removed.song.title.as_str(), removed.origin), selected);
            assert_eq!(occurrences(&queue), others);
        }
    }
}

#[test]
fn invalid_edits_leave_the_queue_unchanged_and_insertion_clamps_to_the_end() {
    let mut queue = PlaybackQueue::from_songs(vec![song("a", "A")], QueueOrigin::Playlist);
    assert!(!queue.move_entry(0, 1));
    assert!(!queue.move_entry(1, 0));
    assert!(queue.remove(usize::MAX).is_none());
    assert_eq!(occurrences(&queue), [("A", QueueOrigin::Playlist)]);
    queue.insert(usize::MAX, vec![song("b", "B")], QueueOrigin::Manual);
    queue.append_entries([QueueEntry::new(song("c", "C"), QueueOrigin::DailyMix)]);
    assert_eq!(
        occurrences(&queue),
        [
            ("A", QueueOrigin::Playlist),
            ("B", QueueOrigin::Manual),
            ("C", QueueOrigin::DailyMix)
        ]
    );
    queue.truncate(usize::MAX);
    assert_eq!(queue.len(), 3);
}

#[test]
fn snapshot_import_normalizes_legacy_and_incomplete_sources() {
    let songs = vec![song("same", "First"), song("same", "Second")];
    let legacy = PlaybackQueue::from_snapshot(songs.clone(), vec![]);
    assert_eq!(
        legacy.snapshot().1,
        [QueueOrigin::Manual, QueueOrigin::Manual]
    );
    let partial = PlaybackQueue::from_snapshot(songs.clone(), vec![QueueOrigin::DailyMix]);
    assert_eq!(
        partial.snapshot().1,
        [QueueOrigin::DailyMix, QueueOrigin::Manual]
    );
    let excess = PlaybackQueue::from_snapshot(
        songs,
        vec![
            QueueOrigin::Playlist,
            QueueOrigin::SongRadio,
            QueueOrigin::AutoContinue,
        ],
    );
    assert_eq!(
        excess.snapshot().1,
        [QueueOrigin::Playlist, QueueOrigin::SongRadio]
    );
    assert_eq!(excess.song_ids(), ["same", "same"]);
    assert!(PlaybackQueue::from_snapshot(vec![], vec![QueueOrigin::Manual]).is_empty());
}

#[test]
fn typed_queues_preserve_the_saved_session_format_and_song_metadata() {
    let mut first = song("repeat", "First occurrence");
    first.album = "Album".into();
    first.duration = Some(123);
    first.cover_art = Some("cover".into());
    let session = PlaybackSession {
        server_url: "https://example.test".into(),
        queue: vec![first, song("repeat", "Second occurrence")],
        queue_origins: vec![QueueOrigin::Playlist, QueueOrigin::ArtistRadio],
        queue_index: Some(1),
        position_secs: 12.5,
        shuffle_played: vec![0, 1],
        shuffle_forward: vec![0],
        ..Default::default()
    };
    let expected = serde_json::to_value(&session).unwrap();
    let mut restored: PlaybackSession = serde_json::from_value(expected.clone()).unwrap();
    let queue = PlaybackQueue::from_snapshot(
        std::mem::take(&mut restored.queue),
        std::mem::take(&mut restored.queue_origins),
    );
    (restored.queue, restored.queue_origins) = queue.snapshot();
    assert_eq!(serde_json::to_value(&restored).unwrap(), expected);
    assert!(expected.get("entries").is_none());

    let mut legacy = expected;
    legacy.as_object_mut().unwrap().remove("queue_origins");
    let legacy: PlaybackSession = serde_json::from_value(legacy).unwrap();
    let queue = PlaybackQueue::from_snapshot(legacy.queue, legacy.queue_origins);
    assert_eq!(queue.origin(0), Some(QueueOrigin::Manual));
    assert_eq!(queue.origin(1), Some(QueueOrigin::Manual));
}

#[test]
fn playback_session_round_trips_through_json() {
    let session = PlaybackSession {
        source: None,
        server_url: "http://localhost:4533".to_string(),
        queue: vec![Song::default()],
        queue_origins: vec![QueueOrigin::DailyMix],
        queue_index: Some(2),
        position_secs: 61.5,
        playback_modes: PlayModes {
            shuffle: true,
            repeat: RepeatMode::All,
        },
        shuffle_played: vec![0, 3, 1],
        shuffle_forward: vec![2],
    };

    let json = serde_json::to_string(&session).expect("session should serialize");
    let restored: PlaybackSession =
        serde_json::from_str(&json).expect("session should deserialize");

    assert_eq!(restored.server_url, session.server_url);
    assert_eq!(restored.queue_origins, vec![QueueOrigin::DailyMix]);
    assert_eq!(restored.queue.len(), 1);
    assert_eq!(restored.queue_index, Some(2));
    assert!((restored.position_secs - 61.5).abs() < 1e-9);
    assert!(restored.playback_modes.shuffle);
    assert_eq!(restored.playback_modes.repeat, RepeatMode::All);
    assert_eq!(restored.shuffle_played, vec![0, 3, 1]);
    assert_eq!(restored.shuffle_forward, vec![2]);
}

#[test]
fn saved_sessions_match_equivalent_http_addresses_but_preserve_server_boundaries() {
    for (saved, current, expected) in [
        ("https://music.example/", "https://music.example", true),
        (
            "http://music.example:80/mount/",
            "music.example/mount",
            true,
        ),
        (
            "https://MUSIC.example:443/Music/",
            " https://music.example/Music/ ",
            true,
        ),
        ("http://[::1]:4533/", "[::1]:4533", true),
        (
            "https://music.example/a%20b/",
            "https://music.example/a%20b",
            true,
        ),
        (
            "https://music.example/Music",
            "https://music.example/music",
            false,
        ),
        (
            "https://music.example/music",
            "https://music.example/other",
            false,
        ),
        ("https://music.example", "http://music.example", false),
        (
            "http://music.example:4533",
            "http://music.example:4534",
            false,
        ),
        ("http://music.example", "http://other.example", false),
        (
            "https://music.example?library=a",
            "https://music.example?library=b",
            false,
        ),
        ("", "", false),
        ("file:///music", "file:///music", false),
        ("http://[broken", "http://[broken", false),
    ] {
        let saved = PlaybackSession {
            server_url: saved.into(),
            ..Default::default()
        };
        assert_eq!(
            saved.matches_server(current),
            expected,
            "{} vs {current}",
            saved.server_url
        );
    }
}

#[test]
fn legacy_queue_origins_default_to_manual_and_length_is_normalized() {
    let legacy = r#"{
            "server_url":"http://localhost:4533",
            "queue":[{"id":"a","title":"A"},{"id":"b","title":"B"}],
            "queue_index":0,
            "position_secs":0,
            "playback_modes":{"shuffle":false,"repeat":"off"},
            "shuffle_played":[],
            "shuffle_forward":[]
        }"#;
    let session: PlaybackSession = serde_json::from_str(legacy).unwrap();
    assert!(session.queue_origins.is_empty());
    assert_eq!(
        normalize_queue_origins(session.queue_origins, session.queue.len()),
        vec![QueueOrigin::Manual, QueueOrigin::Manual]
    );
    assert_eq!(
        normalize_queue_origins(vec![QueueOrigin::SongRadio, QueueOrigin::DailyMix], 1),
        vec![QueueOrigin::SongRadio]
    );
}

#[test]
fn corrupted_session_json_fails_cleanly() {
    assert!(serde_json::from_str::<PlaybackSession>("not json at all").is_err());
}

#[test]
fn queue_identity_survives_edits_and_copies_create_new_occurrences() {
    let mut queue = PlaybackQueue::from_songs(
        vec![song("same", "A"), song("same", "B")],
        QueueOrigin::Manual,
    );
    let first = queue.entry_id(0).unwrap();
    let second = queue.entry_id(1).unwrap();
    assert_ne!(first, second);
    queue.insert(0, vec![song("same", "Inserted")], QueueOrigin::Playlist);
    assert_eq!(queue.entry_id(1), Some(first));
    assert_eq!(queue.entry_id(2), Some(second));
    queue.move_entry(1, 2);
    assert_eq!(queue.entry_id(1), Some(second));
    assert_eq!(queue.entry_id(2), Some(first));
    let copied = queue.entries()[2].clone();
    let copied_id = copied.id();
    assert_ne!(copied_id, first);
    assert_ne!(copied_id, second);
    queue.append_entries([copied]);
    let moved = queue.remove(2).unwrap();
    assert_eq!(moved.id(), first);
    queue.append_entries([moved]);
    assert_eq!(queue.entry_id(queue.len() - 1), Some(first));
    let (songs, origins) = queue.snapshot();
    let restored = PlaybackQueue::from_snapshot(songs, origins);
    assert!(restored
        .entries()
        .iter()
        .all(|entry| queue.entries().iter().all(|old| old.id() != entry.id())));
    assert_eq!(occurrences(&restored), occurrences(&queue));
}

#[test]
fn owned_insertion_moves_song_allocations_and_creates_new_identities_at_every_boundary() {
    for count in [0, 1, 2, 32] {
        for index in [0, 1, 2, 3, usize::MAX] {
            let mut queue = PlaybackQueue::from_songs(
                vec![song("same", "First"), song("same", "Second")],
                QueueOrigin::Playlist,
            );
            let old: Vec<_> = queue.entries().iter().map(QueueEntry::id).collect();
            let songs: Vec<_> = (0..count)
                .map(|index| song("same", &format!("Inserted {index}")))
                .collect();
            let pointers: Vec<_> = songs
                .iter()
                .map(|song| (song.id.as_ptr(), song.title.as_ptr()))
                .collect();
            let titles: Vec<_> = songs.iter().map(|song| song.title.clone()).collect();

            queue.insert(index, songs, QueueOrigin::Manual);

            let start = index.min(old.len());
            assert_eq!(queue.len(), old.len() + count);
            for (offset, ((id, title), expected_title)) in pointers.iter().zip(&titles).enumerate()
            {
                let entry = &queue.entries()[start + offset];
                assert_eq!(entry.song.id.as_ptr(), *id);
                assert_eq!(entry.song.title.as_ptr(), *title);
                assert_eq!(&entry.song.title, expected_title);
                assert_eq!(entry.origin, QueueOrigin::Manual);
                assert!(!old.contains(&entry.id()));
                assert!(queue.entries()[..start + offset]
                    .iter()
                    .all(|other| other.id() != entry.id()));
            }
            let identities: Vec<_> = queue.entries().iter().map(QueueEntry::id).collect();
            assert_eq!(&identities[..start], &old[..start]);
            assert_eq!(&identities[start + count..], &old[start..]);
            assert_eq!(
                queue.origin(if start == 0 { count } else { 0 }),
                Some(QueueOrigin::Playlist)
            );
        }
    }
}
