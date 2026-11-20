use super::*;
use crate::app::library::requests::LibraryRequests;
use crate::models::{Playlist, Song};
use crate::playlist::loading::playlist_update_channel;

fn playlist(id: &str) -> Playlist {
    Playlist {
        id: id.into(),
        name: format!("Folder {id}"),
        ..Default::default()
    }
}

fn begin() -> (PlaylistLibrary, LibraryRequests, LibraryRequest) {
    let mut library = PlaylistLibrary::default();
    let mut requests = LibraryRequests::default();
    library.select(playlist("5"));
    let request = library.begin_detail(&mut requests).unwrap();
    library.apply_detail_update(request, PlaylistUpdate::Metadata(playlist("5")));
    (library, requests, request)
}

#[test]
fn partial_stream_metadata_preserves_the_playlist_summary_until_later_fields_arrive() {
    let mut library = PlaylistLibrary::default();
    let mut requests = LibraryRequests::default();
    library.select(Playlist {
        id: "5".into(),
        name: "Known name".into(),
        song_count: Some(4000),
        duration: Some(123_456),
        owner: Some("alice".into()),
        public: Some(true),
        ..Default::default()
    });
    let request = library.begin_detail(&mut requests).unwrap();
    library.apply_detail_update(
        request,
        PlaylistUpdate::Metadata(Playlist {
            id: "5".into(),
            ..Default::default()
        }),
    );

    let playlist = library.playlist().unwrap();
    assert_eq!(playlist.name, "Known name");
    assert_eq!(playlist.song_count, Some(4000));
    assert_eq!(playlist.duration, Some(123_456));
    assert_eq!(playlist.owner.as_deref(), Some("alice"));
    assert_eq!(playlist.public, Some(true));
}

fn batch(ids: &[&str], count: usize) -> PlaylistUpdate {
    PlaylistUpdate::Batch {
        songs: ids
            .iter()
            .map(|id| Song {
                id: (*id).into(),
                title: (*id).into(),
                cover_art: Some(format!("cover-{id}")),
                ..Default::default()
            })
            .collect(),
        progress: PlaylistProgress {
            songs: count,
            ..Default::default()
        },
    }
}

#[test]
fn preview_appends_in_order_and_only_explicit_completion_enables_full_playback() {
    let (mut library, _, request) = begin();
    assert!(library.detail_loading());
    assert!(!library.detail_ready());
    assert!(library.songs().is_empty());
    assert!(matches!(
        library.apply_detail_update(request, batch(&["a", "b"], 2)),
        Some(PlaylistDetailChange::Updated { reset: true })
    ));
    assert!(!library.detail_ready());
    assert_eq!(library.snapshot(), Some(request));
    assert!(matches!(
        library.apply_detail_update(request, batch(&["a"], 3)),
        Some(PlaylistDetailChange::Updated { reset: false })
    ));
    assert_eq!(crate::playlist::song_ids(library.songs()), ["a", "b", "a"]);
    assert!(library.detail_loading());
    library.apply_detail_update(
        request,
        PlaylistUpdate::Complete(PlaylistProgress {
            songs: 3,
            unavailable_count: 1,
            ..Default::default()
        }),
    );
    assert!(library.detail_ready());
    assert!(!library.detail_loading());
    assert_eq!(library.unavailable_count(), 1);
    assert!(library
        .apply_detail_update(request, batch(&["late"], 4))
        .is_none());
    assert_eq!(library.songs().len(), 3);
}

#[test]
fn retry_replaces_the_previous_preview_and_rejects_old_batches_and_completion() {
    let (mut library, mut requests, old) = begin();
    library.apply_detail_update(old, batch(&["a", "b"], 2));
    assert_eq!(
        library.playlist().unwrap().cover_art.as_deref(),
        Some("cover-a")
    );
    library.apply_detail_update(
        old,
        PlaylistUpdate::Failed(PlaylistLoadError {
            kind: PlaylistFailureKind::Network,
            message: "offline".into(),
        }),
    );
    assert_eq!(library.songs().len(), 2);
    assert!(!library.detail_ready());
    assert_eq!(library.detail_error(), Some("offline"));
    let current = library.begin_detail(&mut requests).unwrap();
    assert_eq!(library.songs().len(), 2);
    assert_eq!(library.snapshot(), Some(old));
    library.apply_detail_update(current, PlaylistUpdate::Metadata(playlist("5")));
    assert_eq!(
        library.playlist().unwrap().cover_art.as_deref(),
        Some("cover-a")
    );
    library.apply_detail_update(current, batch(&["replacement"], 1));
    assert_eq!(
        library.playlist().unwrap().cover_art.as_deref(),
        Some("cover-replacement")
    );
    assert_eq!(crate::playlist::song_ids(library.songs()), ["replacement"]);
    assert_eq!(library.snapshot(), Some(current));
    assert!(library
        .apply_detail_update(old, batch(&["stale"], 3))
        .is_none());
    assert!(library
        .apply_detail_update(
            old,
            PlaylistUpdate::Complete(PlaylistProgress {
                songs: 2,
                ..Default::default()
            })
        )
        .is_none());
    assert!(!library.detail_ready());
    library.apply_detail_update(
        current,
        PlaylistUpdate::Complete(PlaylistProgress {
            songs: 1,
            ..Default::default()
        }),
    );
    assert!(library.detail_ready());
    assert_eq!(crate::playlist::song_ids(library.songs()), ["replacement"]);
}

#[test]
fn a_previous_folder_cannot_append_to_or_complete_the_new_folder() {
    let (mut library, mut requests, old) = begin();
    library.apply_detail_update(old, batch(&["old"], 1));
    library.select(playlist("6"));
    let current = library.begin_detail(&mut requests).unwrap();
    library.apply_detail_update(current, PlaylistUpdate::Metadata(playlist("6")));
    assert!(library
        .apply_detail_update(old, batch(&["stale"], 2))
        .is_none());
    library.apply_detail_update(current, batch(&["new"], 1));
    assert!(library
        .apply_detail_update(
            old,
            PlaylistUpdate::Complete(PlaylistProgress {
                songs: 1,
                ..Default::default()
            })
        )
        .is_none());
    assert_eq!(library.playlist().unwrap().id, "6");
    assert_eq!(crate::playlist::song_ids(library.songs()), ["new"]);
    assert!(!library.detail_ready());
}

#[test]
fn authentication_failure_discards_the_preview_and_its_event_identity() {
    let (mut library, _, request) = begin();
    library.apply_detail_update(request, batch(&["private"], 1));
    library.apply_detail_update(
        request,
        PlaylistUpdate::Failed(PlaylistLoadError {
            kind: PlaylistFailureKind::Authorization,
            message: "expired".into(),
        }),
    );
    assert!(library.songs().is_empty());
    assert_eq!(library.snapshot(), None);
    assert!(!library.detail_ready());
    assert_eq!(library.detail_error(), Some("expired"));
}

#[test]
fn malformed_updates_cannot_mark_a_preview_as_complete() {
    for update in [
        PlaylistUpdate::Metadata(playlist("another-folder")),
        batch(&[], 0),
        batch(&["a"], 2),
        batch(&["a"; PLAYLIST_BATCH_SIZE + 1], PLAYLIST_BATCH_SIZE + 1),
        PlaylistUpdate::Progress(PlaylistProgress {
            songs: 1,
            ..Default::default()
        }),
        PlaylistUpdate::Complete(PlaylistProgress {
            songs: 1,
            ..Default::default()
        }),
    ] {
        let (mut library, _, request) = begin();
        library.apply_detail_update(request, update);
        assert!(!library.detail_ready());
        assert!(!library.detail_loading());
        assert!(library.detail_error().is_some());
        assert!(library.songs().is_empty());
    }
    let (mut library, mut requests, _) = begin();
    let request = library.begin_detail(&mut requests).unwrap();
    library.apply_detail_update(
        request,
        PlaylistUpdate::Complete(PlaylistProgress::default()),
    );
    assert!(
        !library.detail_ready(),
        "a closed or empty stream is not confirmation of an empty folder"
    );
}

#[test]
fn successful_empty_refresh_replaces_old_rows_and_keeps_unavailable_count() {
    let (mut library, mut requests, old) = begin();
    library.apply_detail_update(old, batch(&["old"], 1));
    library.apply_detail_update(
        old,
        PlaylistUpdate::Complete(PlaylistProgress {
            songs: 1,
            ..Default::default()
        }),
    );
    let current = library.begin_detail(&mut requests).unwrap();
    library.apply_detail_update(current, PlaylistUpdate::Metadata(playlist("5")));
    library.apply_detail_update(
        current,
        PlaylistUpdate::Complete(PlaylistProgress {
            unavailable_count: 3,
            ..Default::default()
        }),
    );
    assert!(library.detail_ready());
    assert!(library.songs().is_empty());
    assert_eq!(library.snapshot(), Some(current));
    assert_eq!(library.unavailable_count(), 3);
    assert!(library.playlist().unwrap().cover_art.is_none());
}

#[tokio::test]
async fn disconnect_without_completion_keeps_a_preview_and_reports_failure() {
    let (mut library, _, request) = begin();
    let source = SourceId::new(ProviderKind::Bilibili, "test");
    let (sender, receiver) = playlist_update_channel();
    let worker = tokio::spawn(std::future::pending::<()>());
    library.attach_detail_stream(request, source.clone(), receiver, worker.abort_handle());
    let PlaylistUpdate::Batch {
        mut songs,
        progress,
    } = batch(&["a"], 1)
    else {
        unreachable!()
    };
    songs[0].source = Some(source);
    sender
        .send(PlaylistUpdate::Batch { songs, progress })
        .await
        .unwrap();
    drop(sender);
    while let Some((request, update)) = library.next_detail_update() {
        library.apply_detail_update(request, update);
    }
    assert_eq!(crate::playlist::song_ids(library.songs()), ["a"]);
    assert!(!library.detail_loading());
    assert!(!library.detail_ready());
    assert!(library.detail_error().is_some());
    assert!(worker.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn batches_must_belong_to_the_attached_source_and_part_counts_replace_video_counts() {
    for wrong_source in [false, true] {
        let (mut library, _, request) = begin();
        let source = SourceId::new(ProviderKind::Bilibili, "correct");
        let (sender, receiver) = playlist_update_channel();
        let worker = tokio::spawn(std::future::pending::<()>());
        library.attach_detail_stream(request, source.clone(), receiver, worker.abort_handle());
        let mut metadata = playlist("5");
        metadata.song_count = Some(17);
        library.apply_detail_update(request, PlaylistUpdate::Metadata(metadata));
        let songs = (1..=3)
            .map(|index| Song {
                source: Some(if wrong_source {
                    SourceId::new(ProviderKind::Bilibili, "wrong")
                } else {
                    source.clone()
                }),
                id: format!("part-{index}"),
                duration: Some(index),
                cover_art: Some("cover".into()),
                ..Default::default()
            })
            .collect();
        let progress = PlaylistProgress {
            songs: 3,
            videos: 1,
            pages: 1,
            ..Default::default()
        };
        library.apply_detail_update(request, PlaylistUpdate::Batch { songs, progress });
        if wrong_source {
            assert!(library.detail_error().is_some());
            assert!(library.songs().is_empty());
        } else {
            assert!(!library.detail_ready());
            library.apply_detail_update(request, PlaylistUpdate::Complete(progress));
            assert!(library.detail_ready());
            assert_eq!(library.playlist().unwrap().song_count, Some(3));
            assert_eq!(library.playlist().unwrap().duration, Some(6));
            assert_eq!(
                library.playlist().unwrap().cover_art.as_deref(),
                Some("cover")
            );
        }
        assert!(sender.is_closed());
        assert!(worker.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn refreshing_switching_and_dropping_release_blocked_updates_and_cancel_work() {
    for action in 0..3 {
        let (mut library, mut requests, request) = begin();
        let (sender, receiver) = playlist_update_channel();
        let worker = tokio::spawn(std::future::pending::<()>());
        library.attach_detail_stream(
            request,
            SourceId::new(ProviderKind::Bilibili, "test"),
            receiver,
            worker.abort_handle(),
        );
        for _ in 0..sender.max_capacity() {
            sender
                .send(PlaylistUpdate::Progress(PlaylistProgress::default()))
                .await
                .unwrap();
        }
        assert_eq!(sender.capacity(), 0);
        let blocked_sender = sender.clone();
        let blocked = tokio::spawn(async move {
            blocked_sender
                .send(PlaylistUpdate::Progress(PlaylistProgress::default()))
                .await
        });
        match action {
            0 => {
                library.begin_detail(&mut requests);
            }
            1 => library.select(playlist("other")),
            _ => drop(library),
        }
        assert!(sender.is_closed());
        assert!(blocked.await.unwrap().is_err());
        assert!(worker.await.unwrap_err().is_cancelled());
    }
}
