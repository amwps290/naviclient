use super::*;

fn song() -> Song {
    Song {
        id: "track".into(),
        title: "Track".into(),
        artist: "Artist".into(),
        artist_id: Some("artist-id".into()),
        album: "Album".into(),
        album_id: Some("album-id".into()),
        cover_art: Some("cover-id".into()),
        duration: Some(180),
        path: Some("/large/library/file.flac".into()),
        ..Default::default()
    }
}

#[test]
fn artist_projection_keeps_only_the_navigation_metadata() {
    let song = song();
    let artist = artist_for_song(&song).unwrap();
    assert_eq!(artist.id, "artist-id");
    assert_eq!(artist.name, "Artist");
    assert_eq!(artist.cover_art.as_deref(), Some("cover-id"));
    assert!(artist.album_count.is_none());
    assert_eq!(song.id, "track");
    assert_eq!(song.path.as_deref(), Some("/large/library/file.flac"));
}

#[test]
fn album_projection_preserves_artist_navigation_without_inventing_album_totals() {
    let album = album_for_song(&song()).unwrap();
    assert_eq!(album.id, "album-id");
    assert_eq!(album.name, "Album");
    assert_eq!(album.artist, "Artist");
    assert_eq!(album.artist_id.as_deref(), Some("artist-id"));
    assert_eq!(album.cover_art.as_deref(), Some("cover-id"));
    assert!(album.duration.is_none());
    assert!(album.song_count.is_none());
}

#[test]
fn missing_empty_and_whitespace_ids_do_not_create_navigation_targets() {
    for id in [None, Some(""), Some(" \t\n")] {
        let mut song = song();
        song.artist_id = id.map(str::to_string);
        assert!(artist_for_song(&song).is_none());
        let album = album_for_song(&song).unwrap();
        assert!(album.artist_id.is_none());
        song.album_id = id.map(str::to_string);
        assert!(album_for_song(&song).is_none());
    }
}

#[test]
fn missing_names_and_covers_keep_existing_fallback_behavior() {
    let mut song = song();
    song.album = " \t ".into();
    song.cover_art = None;
    let album = album_for_song(&song).unwrap();
    assert_eq!(album.name, "Unknown album");
    assert!(album.cover_art.is_none());
    assert!(artist_for_song(&song).unwrap().cover_art.is_none());
    song.album = "  Album (Live)  ".into();
    assert_eq!(album_for_song(&song).unwrap().name, "  Album (Live)  ");
}
