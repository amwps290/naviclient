use std::collections::HashMap;

use super::*;
use url::Url;

#[test]
fn recent_album_list_uses_the_recent_type() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let url = api
        .url_for("getAlbumList2", &[("type", "recent"), ("size", "30")])
        .unwrap();
    let parsed = Url::parse(&url).unwrap();
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    assert_eq!(params["type"], "recent");
    assert_eq!(params["size"], "30");
}

#[test]
fn home_album_sections_use_supported_list_types() {
    for list_type in ["frequent", "random"] {
        let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
        let url = api
            .url_for("getAlbumList2", &[("type", list_type), ("size", "30")])
            .unwrap();
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params["type"], list_type);
    }
}

#[test]
fn album_sort_params_map_to_subsonic_list_types() {
    assert_eq!(
        album_sort_params(AlbumSort::NewestAdded),
        vec![("type", "newest".to_string())]
    );
    assert_eq!(
        album_sort_params(AlbumSort::Name),
        vec![("type", "alphabeticalByName".to_string())]
    );
    assert_eq!(
        album_sort_params(AlbumSort::Artist),
        vec![("type", "alphabeticalByArtist".to_string())]
    );
    assert_eq!(
        album_sort_params(AlbumSort::Year),
        vec![
            ("type", "byYear".to_string()),
            ("fromYear", "9999".to_string()),
            ("toYear", "0".to_string()),
        ]
    );
}

#[test]
fn albums_by_genre_uses_subsonic_parameters() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let url = api
        .url_for(
            "getAlbumList2",
            &[
                ("type", "byGenre"),
                ("genre", "Jazz"),
                ("size", "100"),
                ("offset", "300"),
            ],
        )
        .unwrap();
    let parsed = Url::parse(&url).unwrap();
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    assert_eq!(params["type"], "byGenre");
    assert_eq!(params["genre"], "Jazz");
    assert_eq!(params["size"], "100");
    assert_eq!(params["offset"], "300");
}

#[test]
fn artist_top_songs_prefers_stable_ids_only_when_advertised() {
    let artist = Artist {
        id: "artist-7".into(),
        name: "Same Name".into(),
        ..Artist::default()
    };
    assert_eq!(
        artist_top_songs_params(&artist, 12, CapabilitySupport::Supported),
        vec![
            ("artistId", "artist-7".to_string()),
            ("count", "12".to_string())
        ]
    );
    for support in [CapabilitySupport::Unknown, CapabilitySupport::Unsupported] {
        assert_eq!(
            artist_top_songs_params(&artist, 12, support),
            vec![
                ("artist", "Same Name".to_string()),
                ("count", "12".to_string())
            ]
        );
    }

    let blank_id = Artist {
        id: "  ".into(),
        ..artist
    };
    assert_eq!(
        artist_top_songs_params(&blank_id, 0, CapabilitySupport::Supported),
        vec![
            ("artist", "Same Name".to_string()),
            ("count", "1".to_string())
        ]
    );
}

#[test]
fn song_radio_requests_use_standard_subsonic_parameters() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let similar = api
        .url_for("getSimilarSongs2", &[("id", "seed"), ("count", "24")])
        .unwrap();
    let params: HashMap<String, String> = Url::parse(&similar)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(params["id"], "seed");
    assert_eq!(params["count"], "24");

    let genre = api
        .url_for(
            "getSongsByGenre",
            &[("genre", "Jazz & Soul"), ("count", "24"), ("offset", "0")],
        )
        .unwrap();
    let params: HashMap<String, String> = Url::parse(&genre)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(params["genre"], "Jazz & Soul");
    assert_eq!(params["count"], "24");
    assert_eq!(params["offset"], "0");
}

#[test]
fn song_genre_is_parsed_for_explainable_radio_fallback() {
    let song: Song = serde_json::from_value(serde_json::json!({
        "id": "s",
        "title": "Song",
        "genre": "Ambient"
    }))
    .unwrap();
    assert_eq!(song.genre.as_deref(), Some("Ambient"));
}

#[test]
fn genre_parses_subsonic_camel_case_counts() {
    let genre: Genre = serde_json::from_value(serde_json::json!({
        "value": "Progressive Rock",
        "songCount": 42,
        "albumCount": 7,
    }))
    .expect("genre should deserialize");
    assert_eq!(genre.value, "Progressive Rock");
    assert_eq!(genre.song_count, Some(42));
    assert_eq!(genre.album_count, Some(7));

    let minimal: Genre = serde_json::from_value(serde_json::json!({"value": "Rock"}))
        .expect("genre without counts should deserialize");
    assert_eq!(minimal.song_count, None);
    assert_eq!(minimal.album_count, None);
}

#[test]
fn sorted_album_request_includes_type_size_and_offset() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let mut params = album_sort_params(AlbumSort::Artist);
    params.push(("size", "100".to_string()));
    params.push(("offset", "300".to_string()));
    let params: Vec<(&str, &str)> = params
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let url = api.url_for("getAlbumList2", &params).unwrap();
    let parsed = Url::parse(&url).unwrap();
    let query: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    assert_eq!(query["type"], "alphabeticalByArtist");
    assert_eq!(query["size"], "100");
    assert_eq!(query["offset"], "300");
}

#[test]
fn uses_subsonic_favorite_parameter_names() {
    assert_eq!(favorite_param(FavoriteKind::Artist), "artistId");
    assert_eq!(favorite_param(FavoriteKind::Album), "albumId");
    assert_eq!(favorite_param(FavoriteKind::Song), "id");
}
