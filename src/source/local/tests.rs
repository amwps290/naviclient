use std::fs;
use std::path::Path;

use super::*;
use crate::models::{AlbumSort, FavoriteKey, FavoriteKind};
use crate::source::{ProviderKind, SourceId};

/// Write a minimal valid 1-second silent 44.1kHz stereo 16-bit PCM WAV file.
fn write_test_wav(path: &Path) {
    let sample_rate: u32 = 44_100;
    let channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let num_samples: u32 = sample_rate; // 1 second
    let data_size: u32 = num_samples * u32::from(channels) * u32::from(bits_per_sample / 8);
    let byte_rate: u32 = sample_rate * u32::from(channels) * u32::from(bits_per_sample / 8);
    let block_align: u16 = channels * (bits_per_sample / 8);

    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.resize(44 + data_size as usize, 0);
    fs::write(path, bytes).unwrap();
}

#[test]
fn scans_incremental_persists_favorites_lyrics_and_resolves_local_file() {
    let dir = tempfile::tempdir().unwrap();
    let music_root = dir.path().join("music");
    let album_dir = music_root.join("Sunrise Album");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&album_dir).unwrap();

    let track_a = album_dir.join("01 Morning Light.wav");
    let track_b = album_dir.join("02 Evening Breeze.wav");
    write_test_wav(&track_a);
    write_test_wav(&track_b);
    fs::write(album_dir.join("cover.jpg"), b"fake-jpeg-bytes").unwrap();
    fs::write(
        album_dir.join("01 Morning Light.lrc"),
        "[00:01.00]Hello sunrise\n[00:02.50]Warm light\n",
    )
    .unwrap();

    let source_id = SourceId::new(ProviderKind::LocalFolder, "local-1");
    let provider = LocalFolderProvider::with_data_dir(
        source_id.clone(),
        "My Folder".into(),
        vec![music_root.clone()],
        data_dir.clone(),
    );

    let first = provider.rescan().unwrap();
    assert_eq!(first.total_tracks, 2);
    assert_eq!(first.parsed_tracks, 2);
    assert_eq!(first.reused_tracks, 0);

    // Second scan with unchanged mtime/size should reuse both tracks.
    let second = provider.rescan().unwrap();
    assert_eq!(second.total_tracks, 2);
    assert_eq!(second.parsed_tracks, 0);
    assert_eq!(second.reused_tracks, 2);

    // Reloading from disk restores the persisted index immediately.
    let reloaded = LocalFolderProvider::with_data_dir(
        source_id.clone(),
        "My Folder".into(),
        vec![music_root.clone()],
        data_dir.clone(),
    );
    assert_eq!(reloaded.track_count(), 2);

    let albums = reloaded.albums(20, 0, AlbumSort::Name);
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "Sunrise Album");
    assert_eq!(albums[0].song_count, Some(2));
    assert!(albums[0].cover_art.is_some());

    // Cover bytes load from folder cover file.
    let cover_url = reloaded
        .cover_url(albums[0].cover_art.as_deref().unwrap())
        .unwrap();
    assert_eq!(reloaded.cover_bytes(&cover_url).unwrap(), b"fake-jpeg-bytes");

    let songs = reloaded.album_songs(&albums[0].id);
    assert_eq!(songs.len(), 2);
    let morning = songs
        .iter()
        .find(|s| s.title.contains("Morning Light"))
        .unwrap();
    assert_eq!(morning.duration, Some(1));
    assert_eq!(morning.suffix.as_deref(), Some("wav"));

    // Sibling .lrc lyrics are parsed.
    let lyrics = reloaded.lyrics(morning).unwrap();
    assert!(lyrics.is_synced());
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "Hello sunrise");

    // Local favorites persist across reloads.
    reloaded
        .set_favorite(&FavoriteKey::new(FavoriteKind::Song, &morning.id), true)
        .unwrap();
    let reloaded_again = LocalFolderProvider::with_data_dir(
        source_id,
        "My Folder".into(),
        vec![music_root],
        data_dir,
    );
    let favs = reloaded_again.favorites();
    assert_eq!(favs.songs.len(), 1);
    assert_eq!(favs.songs[0].id, morning.id);

    // Search matches song titles.
    let found = reloaded_again.search("breeze", 10, 10, 10);
    assert_eq!(found.songs.len(), 1);
    assert!(found.songs[0].title.contains("Evening Breeze"));

    // Playback resolution returns a local-file HttpResource.
    let playback = reloaded_again.resolve_playback(morning).unwrap();
    assert_eq!(playback.http.local_path(), Some(track_a.as_path()));
    assert_eq!(playback.http.mime_hint(), Some("audio/wav"));
}
