use std::fs;

use super::store::OfflineEntry;
use super::*;
use crate::models::{AlbumSort, FavoriteKey, FavoriteKind, Playlist, Song, TranscodingQuality};
use crate::source::{MediaKey, ProviderKind, SourceId};

fn sample_song(source: &SourceId, id: &str, title: &str, album: &str, artist: &str) -> Song {
    Song {
        source: Some(source.clone()),
        id: id.into(),
        title: title.into(),
        album: album.into(),
        album_id: Some(format!("alb-{album}")),
        artist: artist.into(),
        artist_id: Some(format!("art-{artist}")),
        genre: Some("Ambient".into()),
        track: Some(1),
        duration: Some(180),
        suffix: Some("flac".into()),
        ..Default::default()
    }
}

#[test]
fn offline_store_persists_queries_and_removes_protected_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let offline_dir = dir.path().join("offline");
    fs::create_dir_all(&offline_dir).unwrap();

    let source = SourceId::new(ProviderKind::OpenSubsonic, "srv-1");
    let song_a = sample_song(&source, "s1", "Dawn", "Horizons", "Aether");
    let song_b = sample_song(&source, "s2", "Dusk", "Horizons", "Aether");
    let media_a = MediaKey::new(source.clone(), "s1");
    let media_b = MediaKey::new(source.clone(), "s2");

    let file_a = format!("{}.flac", media_a.cache_key());
    let file_b = format!("{}.flac", media_b.cache_key());
    fs::write(offline_dir.join(&file_a), vec![1u8; 2048]).unwrap();
    fs::write(offline_dir.join(&file_b), vec![2u8; 4096]).unwrap();

    let mut store = OfflineStore::open(offline_dir.clone());
    assert!(store.mark_downloading(media_a.clone()));
    assert_eq!(store.status(&media_a), OfflineTrackStatus::Downloading);
    store
        .finish_download(
            &media_a,
            Some(OfflineEntry {
                media: media_a.clone(),
                song: song_a.clone(),
                file_name: file_a.clone(),
                quality: TranscodingQuality::Original,
                bytes: 2048,
                pinned_at: 100,
            }),
        )
        .unwrap();
    store
        .finish_download(
            &media_b,
            Some(OfflineEntry {
                media: media_b.clone(),
                song: song_b.clone(),
                file_name: file_b.clone(),
                quality: TranscodingQuality::Original,
                bytes: 4096,
                pinned_at: 200,
            }),
        )
        .unwrap();
    store
        .record_playlist_snapshot(
            source.clone(),
            Playlist {
                id: "pl-1".into(),
                name: "Evening Mix".into(),
                ..Default::default()
            },
            &[song_a.clone(), song_b.clone()],
        )
        .unwrap();

    // Reload from disk and verify offline browse/search/playlist queries.
    let mut reloaded = OfflineStore::open(offline_dir.clone());
    assert_eq!(reloaded.status(&media_a), OfflineTrackStatus::Ready);
    assert!(reloaded.audio_path(&media_a).is_some());
    let stats = reloaded.stats(Some(&source));
    assert_eq!(stats.track_count, 2);
    assert_eq!(stats.total_bytes, 6144);
    assert_eq!(reloaded.artists(&source).len(), 1);
    assert_eq!(reloaded.albums(&source, AlbumSort::NewestAdded).len(), 1);
    assert_eq!(reloaded.album_songs(&source, "alb-Horizons").len(), 2);
    assert_eq!(reloaded.genres(&source).len(), 1);
    assert_eq!(reloaded.playlists(&source).len(), 1);
    assert_eq!(reloaded.playlist_songs(&source, "pl-1").len(), 2);

    let search = reloaded.search(&source, "dusk");
    assert_eq!(search.songs.len(), 1);
    assert_eq!(search.songs[0].id, "s2");

    // Removing a track deletes its file on disk.
    assert!(reloaded.remove_track(&media_a).unwrap());
    assert!(!offline_dir.join(&file_a).exists());
    assert_eq!(reloaded.stats(None).track_count, 1);

    // Clear all removes everything.
    let (removed_count, removed_bytes) = reloaded.clear_all().unwrap();
    assert_eq!(removed_count, 1);
    assert_eq!(removed_bytes, 4096);
    assert!(!offline_dir.join(&file_b).exists());
}

#[test]
fn offline_outbox_coalesces_favorite_toggles_and_drains_per_source() {
    let dir = tempfile::tempdir().unwrap();
    let src_a = SourceId::new(ProviderKind::OpenSubsonic, "a");
    let src_b = SourceId::new(ProviderKind::OpenSubsonic, "b");
    let key = FavoriteKey::new(FavoriteKind::Song, "song-1");

    let mut outbox = OfflineOutbox::open(dir.path());
    outbox
        .push(PendingSyncOp::favorite(src_a.clone(), &key, true))
        .unwrap();
    outbox
        .push(PendingSyncOp::favorite(src_a.clone(), &key, false))
        .unwrap();
    outbox
        .push(PendingSyncOp::Scrobble {
            source: src_b.clone(),
            song_id: "song-2".into(),
        })
        .unwrap();

    let mut reloaded = OfflineOutbox::open(dir.path());
    assert_eq!(reloaded.len(), 2);
    let a_ops = reloaded.take_for_source(&src_a);
    assert_eq!(a_ops.len(), 1);
    assert!(matches!(
        a_ops[0],
        PendingSyncOp::SetFavorite { starred: false, .. }
    ));
    assert_eq!(reloaded.len(), 1);
}
