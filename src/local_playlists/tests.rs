use std::fs;
use std::path::PathBuf;

use crate::local_playlists::m3u8::export_m3u8;
use crate::local_playlists::{external_files_source_id, import_m3u8, LocalPlaylistStore};
use crate::models::Song;
use crate::source::{ProviderKind, SourceId};

fn sample_song(source: SourceId, id: &str, title: &str, artist: &str, duration: i32) -> Song {
    Song {
        source: Some(source),
        id: id.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        album: "Test Album".to_string(),
        duration: Some(duration),
        ..Default::default()
    }
}

#[test]
fn store_crud_reorder_and_persistence_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let subsonic = SourceId::new(ProviderKind::OpenSubsonic, "home-nas");
    let local = SourceId::new(ProviderKind::LocalFolder, "laptop");
    let webdav = SourceId::new(ProviderKind::WebDav, "alist");

    let mut store = LocalPlaylistStore::open(dir.path().to_path_buf());
    assert!(store.list().is_empty());

    let s1 = sample_song(subsonic.clone(), "s1", "First", "Artist A", 200);
    let s2 = sample_song(local.clone(), "s2", "Second", "Artist B", 180);
    let s3 = sample_song(webdav.clone(), "s3", "Third", "Artist C", 240);

    let created = store.create("Road Trip", vec![s1.clone(), s2.clone()]).unwrap();
    assert_eq!(created.name, "Road Trip");
    assert_eq!(created.song_count(), 2);
    assert!(created.is_cross_source());
    assert_eq!(created.total_duration_secs(), Some(380));

    let appended = store.append_songs(&created.id, vec![s3.clone()]).unwrap();
    assert_eq!(appended.song_count(), 3);
    assert_eq!(appended.distinct_sources().len(), 3);

    let moved = store.move_song(&created.id, 2, 0).unwrap();
    assert_eq!(moved.songs[0].id, "s3");
    assert_eq!(moved.songs[1].id, "s1");
    assert_eq!(moved.songs[2].id, "s2");

    let renamed = store.rename(&created.id, "Night Drive").unwrap();
    assert_eq!(renamed.name, "Night Drive");

    let removed = store.remove_song(&created.id, 1).unwrap();
    assert_eq!(removed.songs.len(), 2);
    assert_eq!(removed.songs[0].id, "s3");
    assert_eq!(removed.songs[1].id, "s2");

    // Reopen from disk and confirm full fidelity.
    let reopened = LocalPlaylistStore::open(dir.path().to_path_buf());
    assert_eq!(reopened.list().len(), 1);
    let loaded = reopened.get(&created.id).unwrap();
    assert_eq!(loaded.name, "Night Drive");
    assert_eq!(loaded.songs.len(), 2);
    assert_eq!(loaded.songs[0].source, Some(webdav));
    assert_eq!(loaded.songs[1].source, Some(local));

    let mut reopened = reopened;
    reopened.delete(&created.id).unwrap();
    assert!(reopened.list().is_empty());
    let after_delete = LocalPlaylistStore::open(dir.path().to_path_buf());
    assert!(after_delete.list().is_empty());
}

#[test]
fn m3u8_roundtrip_preserves_cross_source_tracks_and_resolves_local_paths() {
    let dir = tempfile::tempdir().unwrap();
    let music_dir = dir.path().join("music");
    fs::create_dir_all(&music_dir).unwrap();
    let local_file = music_dir.join("track.mp3");
    fs::write(&local_file, b"fake mp3 bytes").unwrap();

    let subsonic = SourceId::new(ProviderKind::OpenSubsonic, "srv-1");
    let local_source = SourceId::new(ProviderKind::LocalFolder, "local-1");
    let bilibili = SourceId::new(ProviderKind::Bilibili, "bili-1");

    let mut local_song = sample_song(local_source.clone(), "l1", "Local Song", "Local Artist", 150);
    local_song.path = Some(local_file.to_string_lossy().into_owned());
    let remote_song = sample_song(subsonic.clone(), "r1", "Remote Song", "Remote Artist", 210);
    let bili_song = sample_song(bilibili.clone(), "BV1xx411c7mD:1", "Bili Track", "UP", 300);

    let store_dir = dir.path().join("store");
    let mut store = LocalPlaylistStore::open(store_dir);
    let playlist = store
        .create("Cross Source Mix", vec![local_song, remote_song, bili_song])
        .unwrap();

    let exported = export_m3u8(&playlist, |song| {
        song.path.as_deref().map(PathBuf::from).filter(|p| p.is_file())
    });
    assert!(exported.starts_with("#EXTM3U\n"));
    assert!(exported.contains("#PLAYLIST:Cross Source Mix\n"));
    assert!(exported.contains(&local_file.to_string_lossy().into_owned()));
    assert!(exported.contains("naviclient://opensubsonic/srv-1/r1"));
    assert!(exported.contains("naviclient://bilibili/bili-1/BV1xx411c7mD%3A1"));

    let m3u8_path = dir.path().join("mix.m3u8");
    fs::write(&m3u8_path, exported).unwrap();

    let imported = import_m3u8(&m3u8_path, &[(local_source.clone(), vec![music_dir.clone()])]).unwrap();
    assert_eq!(imported.name, "Cross Source Mix");
    assert_eq!(imported.skipped_missing, 0);
    assert_eq!(imported.songs.len(), 3);
    assert_eq!(imported.songs[0].source, Some(local_source));
    assert_eq!(imported.songs[1].source, Some(subsonic));
    assert_eq!(imported.songs[1].id, "r1");
    assert_eq!(imported.songs[2].source, Some(bilibili));
    assert_eq!(imported.songs[2].id, "BV1xx411c7mD:1");
}

#[test]
fn m3u8_imports_external_plain_playlist_with_relative_and_missing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let audio_dir = dir.path().join("albums");
    fs::create_dir_all(&audio_dir).unwrap();
    let existing = audio_dir.join("01-hello.flac");
    fs::write(&existing, b"not a real flac").unwrap();

    let playlist_file = dir.path().join("Evening.m3u8");
    let text = "\u{feff}#EXTM3U\n#EXTINF:245,Chill Band - Midnight Walk\nalbums/01-hello.flac\n#EXTINF:120,Missing - Ghost\nalbums/missing.mp3\n";
    fs::write(&playlist_file, text).unwrap();

    // Without configured local_sources, falls back to external_files_source_id.
    let imported = import_m3u8(&playlist_file, &[]).unwrap();
    assert_eq!(imported.name, "Evening");
    assert_eq!(imported.songs.len(), 1);
    assert_eq!(imported.skipped_missing, 1);
    assert_eq!(imported.songs[0].title, "Midnight Walk");
    assert_eq!(imported.songs[0].artist, "Chill Band");
    assert_eq!(imported.songs[0].duration, Some(245));
    assert_eq!(imported.songs[0].source, Some(external_files_source_id()));
    assert!(imported.songs[0].path.is_some());

    // With a matching configured local folder source, binds to that source.
    let local_id = SourceId::new(ProviderKind::LocalFolder, "my-music");
    let matched = import_m3u8(&playlist_file, &[(local_id.clone(), vec![audio_dir])]).unwrap();
    assert_eq!(matched.songs[0].source, Some(local_id));
}
