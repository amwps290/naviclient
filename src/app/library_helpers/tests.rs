use super::*;

#[test]
fn album_pagination_offsets_are_contiguous() {
    assert_eq!(album_page_offset(0, 100), 0);
    assert_eq!(album_page_offset(1, 100), 100);
    assert_eq!(album_page_offset(2, 100), 200);
    assert_eq!(album_page_offset(3, 50), 150);
}

fn test_artist(name: &str, album_count: i32) -> Artist {
    Artist {
        id: name.to_string(),
        name: name.to_string(),
        album_count: Some(album_count),
        cover_art: None,
    }
}

#[test]
fn artist_sort_name_is_case_insensitive_ascending() {
    let mut artists = vec![
        test_artist("beatles", 1),
        test_artist("Adele", 1),
        test_artist("coldplay", 1),
    ];
    sort_artists(&mut artists, ArtistSort::Name);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Adele", "beatles", "coldplay"]);
}

#[test]
fn artist_sort_name_desc_reverses_ascending_order() {
    let mut artists = vec![
        test_artist("Adele", 1),
        test_artist("beatles", 1),
        test_artist("coldplay", 1),
    ];
    sort_artists(&mut artists, ArtistSort::NameDesc);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["coldplay", "beatles", "Adele"]);
}

#[test]
fn artist_sort_album_count_orders_by_size_then_name() {
    let mut artists = vec![
        test_artist("Adele", 4),
        test_artist("coldplay", 9),
        test_artist("beatles", 9),
        test_artist("unknown", 0),
    ];
    sort_artists(&mut artists, ArtistSort::AlbumCount);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["beatles", "coldplay", "Adele", "unknown"]);
}

#[test]
fn artist_sort_handles_missing_album_counts() {
    let mut artists = vec![
        Artist {
            id: "a".to_string(),
            name: "A".to_string(),
            album_count: None,
            cover_art: None,
        },
        test_artist("B", 2),
    ];
    sort_artists(&mut artists, ArtistSort::AlbumCount);
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["B", "A"]);
}

fn album_song(title: &str, disc: Option<i32>, track: Option<i32>) -> Song {
    Song {
        id: title.to_string(),
        title: title.to_string(),
        disc,
        track,
        ..Song::default()
    }
}

#[test]
fn album_songs_sort_by_disc_then_track() {
    let mut songs = vec![
        album_song("B1", Some(2), Some(1)),
        album_song("A3", Some(1), Some(3)),
        album_song("A1", Some(1), Some(1)),
        album_song("A2", Some(1), Some(2)),
        album_song("B2", Some(2), Some(2)),
    ];
    sort_album_songs(&mut songs);
    let titles: Vec<&str> = songs.iter().map(|song| song.title.as_str()).collect();
    assert_eq!(titles, ["A1", "A2", "A3", "B1", "B2"]);
}

#[test]
fn album_songs_without_disc_or_track_sort_last_and_stably() {
    let mut songs = vec![
        album_song("Unknown", None, None),
        album_song("Track 2", None, Some(2)),
        album_song("Track 1", Some(1), Some(1)),
    ];
    sort_album_songs(&mut songs);
    let titles: Vec<&str> = songs.iter().map(|song| song.title.as_str()).collect();
    // 缺失碟号/曲目号的排在有效条目之后；Track 2 虽有曲目号但无碟号，
    // 与 Unknown 一样进入“碟号缺失”分组，再按（曲目号, 标题）排。
    assert_eq!(titles, ["Track 1", "Track 2", "Unknown"]);
}
