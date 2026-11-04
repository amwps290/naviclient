use std::collections::HashMap;
use std::time::Duration;

use super::*;

#[test]
fn formats_duration() {
    assert_eq!(format_duration(Some(65)), "1:05");
    assert_eq!(format_duration(Some(3661)), "1:01:01");
    assert_eq!(format_duration(None), "0:00");
}

#[test]
fn builds_subsonic_auth_query() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let url = api.url_for("ping", &[]).unwrap();
    let parsed = Url::parse(&url).unwrap();
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    assert_eq!(params["u"], "alice");
    assert_eq!(params["v"], "1.16.1");
    assert_eq!(params["c"], "navidrome-client");
    assert_eq!(params["f"], "json");
    assert_eq!(params["t"].len(), 32);
    assert_eq!(params["s"].len(), 16);
}

#[test]
fn parses_server_identity_and_open_subsonic_extensions() {
    let ping = serde_json::json!({
        "status": "ok",
        "version": "1.16.1",
        "type": "ExampleSonic",
        "serverVersion": "2.4.0",
        "openSubsonic": true
    });
    let mut info = parse_server_info(&ping);
    assert_eq!(
        info.capabilities.structured_lyrics,
        CapabilitySupport::Unknown
    );
    let capabilities = serde_json::json!({
        "status": "ok",
        "version": "1.16.1",
        "type": "ExampleSonic",
        "serverVersion": "2.4.1",
        "openSubsonicExtensions": [
            {"name": "sonicSimilarity", "versions": [1]},
            {"name": "songLyrics", "versions": [2, 1]},
            {"name": "songLyrics", "versions": [2]},
            {"name": "", "versions": [1]},
            {"versions": [1]}
        ]
    });

    merge_open_subsonic_capabilities(&mut info, &capabilities);

    assert_eq!(info.app_name.as_deref(), Some("ExampleSonic"));
    assert_eq!(info.server_version.as_deref(), Some("2.4.1"));
    assert_eq!(info.protocol_version, "1.16.1");
    assert!(info.open_subsonic);
    assert!(info.extensions_discovered);
    assert!(info.supports_extension("sonicSimilarity", 1));
    assert!(info.supports_extension("SONGLYRICS", 2));
    assert!(!info.supports_extension("songLyrics", 3));
    assert_eq!(
        info.capabilities.structured_lyrics,
        CapabilitySupport::Supported
    );
    assert_eq!(
        info.capabilities.enhanced_lyrics,
        CapabilitySupport::Supported
    );
    assert_eq!(
        info.capabilities.sonic_similarity,
        CapabilitySupport::Supported
    );
    assert_eq!(
        info.capabilities.top_songs_by_artist_id,
        CapabilitySupport::Unsupported
    );
    assert_eq!(
        info.extension_summary(6).as_deref(),
        Some("songLyrics v1/2, sonicSimilarity v1")
    );

    let mut connected = parse_server_info(&serde_json::json!({
        "status": "ok",
        "version": "1.16.1"
    }));
    connected.merge_open_subsonic_info(info.clone());
    assert_eq!(connected, info);
}

#[test]
fn legacy_ping_defaults_to_subsonic_compatibility_mode() {
    let info = parse_server_info(&serde_json::json!({
        "status": "ok",
        "version": "1.13.0",
        "app": "Legacy Server"
    }));

    assert_eq!(info.app_name.as_deref(), Some("Legacy Server"));
    assert_eq!(info.display_version(), "1.13.0");
    assert!(!info.open_subsonic);
    assert!(!info.extensions_discovered);
    assert!(info.extensions.is_empty());
    assert_eq!(
        info.capabilities.structured_lyrics,
        CapabilitySupport::Unknown
    );
    assert_eq!(
        info.capabilities.internet_radio,
        CapabilitySupport::Supported
    );
    assert_eq!(info.extension_summary(4), None);
}

#[test]
fn update_now_playing_uses_subsonic_parameters() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let url = api
        .url_for("updateNowPlaying", &[("id", "song-9"), ("time", "42")])
        .unwrap();
    let parsed = Url::parse(&url).unwrap();
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    assert_eq!(params["id"], "song-9");
    assert_eq!(params["time"], "42");
}

#[test]
fn scrobble_marks_completed_playback() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let url = api
        .url_for("scrobble", &[("id", "song-9"), ("submission", "true")])
        .unwrap();
    let parsed = Url::parse(&url).unwrap();
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
    assert_eq!(params["id"], "song-9");
    assert_eq!(params["submission"], "true");
}

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
fn stream_url_includes_optional_max_bit_rate() {
    let api = OpenSubsonicProvider::new("http://example.test", "alice", "secret").unwrap();
    let url = api.stream_url("song-1", Some(320)).unwrap();
    let parsed = Url::parse(&url).unwrap();
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    assert_eq!(params["id"], "song-1");
    assert_eq!(params["maxBitRate"], "320");
    assert_eq!(params["format"], "mp3");
    assert_eq!(params["estimateContentLength"], "true");
}

#[test]
fn accepts_urls_without_scheme() {
    let api = OpenSubsonicProvider::new("localhost:4533", "alice", "secret").unwrap();
    assert_eq!(api.base_url(), "http://localhost:4533");
}

#[test]
fn uses_subsonic_favorite_parameter_names() {
    assert_eq!(favorite_param(FavoriteKind::Artist), "artistId");
    assert_eq!(favorite_param(FavoriteKind::Album), "albumId");
    assert_eq!(favorite_param(FavoriteKind::Song), "id");
}

fn test_lyric_line(start_ms: Option<u64>, text: &str) -> LyricLine {
    LyricLine {
        start_ms,
        text: text.to_string(),
        translation: None,
    }
}

#[test]
fn translations_pair_by_nearest_timestamp_within_tolerance() {
    let mut primary = vec![
        test_lyric_line(Some(0), "hello"),
        test_lyric_line(Some(5000), "world"),
    ];
    let candidate = vec![
        test_lyric_line(Some(100), "你好"),
        test_lyric_line(Some(5100), "世界"),
        test_lyric_line(Some(9000), "多余行"),
    ];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation.as_deref(), Some("你好"));
    assert_eq!(primary[1].translation.as_deref(), Some("世界"));
}

#[test]
fn translations_reject_far_and_duplicate_and_empty_text() {
    let mut primary = vec![
        test_lyric_line(Some(0), "a"),
        test_lyric_line(Some(10_000), "b"),
        test_lyric_line(Some(20_000), "same"),
    ];
    let candidate = vec![
        test_lyric_line(Some(30_000), "超出容差"),
        test_lyric_line(Some(20_000), "same"),
        test_lyric_line(Some(10_000), ""),
    ];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation, None);
    // 空文本不作为翻译。
    assert_eq!(primary[1].translation, None);
    // 与原文相同也不重复显示。
    assert_eq!(primary[2].translation, None);
}

#[test]
fn unsynced_translations_pair_by_line_index_only() {
    let mut primary = vec![test_lyric_line(None, "one"), test_lyric_line(None, "two")];
    let candidate = vec![test_lyric_line(None, "一"), test_lyric_line(None, "二")];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation.as_deref(), Some("一"));
    assert_eq!(primary[1].translation.as_deref(), Some("二"));

    // 行数不同时不做行号对齐，宁缺毋滥。
    let mut primary = vec![test_lyric_line(None, "one")];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation, None);
}

#[test]
fn structured_lyrics_translation_variant_is_extracted() {
    let body = serde_json::json!({
        "lyricsList": { "structuredLyrics": [
            {
                "lang": "eng",
                "synced": true,
                "line": [
                    {"start": 0, "value": "hello"},
                    {"start": 5000, "value": "world"},
                ]
            },
            {
                "lang": "zho",
                "synced": true,
                "line": [
                    {"start": 0, "value": "你好"},
                    {"start": 5000, "value": "世界"},
                ]
            },
        ]}
    });
    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines[0].text, "hello");
    assert_eq!(lyrics.lines[0].translation.as_deref(), Some("你好"));
    assert_eq!(lyrics.lines[1].text, "world");
    assert_eq!(lyrics.lines[1].translation.as_deref(), Some("世界"));
}

#[test]
fn same_language_variants_do_not_become_translations() {
    let body = serde_json::json!({
        "lyricsList": { "structuredLyrics": [
            {
                "lang": "eng",
                "synced": true,
                "line": [{"start": 0, "value": "hello"}]
            },
            {
                "lang": "eng",
                "synced": false,
                "line": [{"value": "hello (alternate)"}]
            },
        ]}
    });
    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines[0].text, "hello");
    assert_eq!(lyrics.lines[0].translation, None);
}

#[test]
fn parses_synced_structured_lyrics_with_offset() {
    let body = serde_json::json!({
        "lyricsList": {
            "structuredLyrics": [{
                "synced": true,
                "offset": -100,
                "line": [
                    {"start": 1000, "value": "First line"},
                    {"start": 2500, "value": "Second line"}
                ]
            }]
        }
    });

    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert!(lyrics.is_synced());
    assert_eq!(lyrics.lines[0].start_ms, Some(900));
    assert_eq!(
        lyrics.active_line_index(Duration::from_millis(2_000)),
        Some(0)
    );
    assert_eq!(
        lyrics.active_line_index(Duration::from_millis(3_000)),
        Some(1)
    );
}

#[test]
fn structured_lyrics_drop_all_blank_rows() {
    let body = serde_json::json!({
        "lyricsList": {
            "structuredLyrics": [{
                "synced": true,
                "line": [
                    {"start": 0, "value": ""},
                    {"start": 1000, "value": " First line "},
                    {"start": 2000, "value": "   "},
                    {"start": 2500, "value": ""},
                    {"start": 3000, "value": "Second line"},
                    {"start": 4000, "value": ""}
                ]
            }]
        }
    });

    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "First line");
    assert_eq!(lyrics.lines[1].text, "Second line");
}

#[test]
fn structured_variant_ranking_counts_content_instead_of_blank_rows() {
    let body = serde_json::json!({
        "lyricsList": { "structuredLyrics": [
            {
                "lang": "zho",
                "synced": true,
                "line": [
                    {"start": 0, "value": "Only real line"},
                    {"start": 1000, "value": ""},
                    {"start": 2000, "value": ""},
                    {"start": 3000, "value": ""}
                ]
            },
            {
                "lang": "eng",
                "synced": true,
                "line": [
                    {"start": 0, "value": "First real line"},
                    {"start": 1000, "value": "Second real line"}
                ]
            }
        ]}
    });

    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "First real line");
    assert_eq!(lyrics.lines[1].text, "Second real line");
}

#[test]
fn parses_plain_lyrics_lines() {
    let body = serde_json::json!({
        "lyrics": {
            "artist": "Artist",
            "title": "Song",
            "value": " First line \n\n   \nSecond line"
        }
    });

    let lyrics = parse_plain_lyrics(&body).expect("plain lyrics should parse");
    assert!(!lyrics.is_synced());
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "First line");
    assert_eq!(lyrics.lines[1].text, "Second line");
}

#[test]
fn blank_synced_lines_never_receive_translation_text() {
    let mut primary = vec![
        test_lyric_line(Some(0), "original"),
        test_lyric_line(Some(5_000), ""),
    ];
    let candidate = vec![
        test_lyric_line(Some(0), "翻译"),
        test_lyric_line(Some(5_000), "不应写入间奏行"),
    ];

    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation.as_deref(), Some("翻译"));
    assert_eq!(primary[1].translation, None);
}
