use super::*;
use serde_json::json;

#[test]
fn parses_track_metadata_defensively() {
    let track = json!({
        "name": "晴天",
        "artists": [{"name": "周杰伦"}, {"name": ""}],
        "album": {"name": "叶惠美"},
        "duration": 269_000,
    });
    let parsed = parse_track(&track).expect("valid track");
    assert_eq!(parsed.title, "晴天");
    assert_eq!(parsed.artists, vec!["周杰伦"]);
    assert_eq!(parsed.album, "叶惠美");
    assert_eq!(parsed.duration_ms, 269_000);

    let string_duration = json!({
        "name": "晴天",
        "artists": [{"name": "周杰伦"}],
        "duration": "269000",
    });
    assert_eq!(
        parse_track(&string_duration)
            .expect("string duration")
            .duration_ms,
        269_000
    );

    let modern = json!({
        "name": "夜曲",
        "ar": [{"name": "周杰伦"}],
        "al": {"name": "十一月的萧邦"},
        "dt": 226_000,
    });
    let parsed = parse_track(&modern).expect("modern track fields");
    assert_eq!(parsed.artists, vec!["周杰伦"]);
    assert_eq!(parsed.album, "十一月的萧邦");
    assert_eq!(parsed.duration_ms, 226_000);
}

#[test]
fn rejects_truncated_or_partially_unparseable_playlists() {
    let track = json!({
        "name": "晴天",
        "artists": [{"name": "周杰伦"}],
        "duration": 269_000,
    });
    let truncated = json!({
        "result": {"name": "测试", "trackCount": 2, "tracks": [track.clone()]}
    });
    let error = parse_response(&truncated).expect_err("truncated response");
    assert!(error.to_string().contains("1/2"));

    let unavailable = json!({
        "playlist": {
            "name": "测试",
            "trackCount": 2,
            "tracks": [track.clone()]
        }
    });
    let parsed = parse_response_with_unavailable(&unavailable, 1)
        .expect("known unavailable track should not reject the playlist");
    assert_eq!(parsed.tracks.len(), 1);
    assert_eq!(parsed.unavailable_tracks, 1);

    let malformed = json!({
        "result": {
            "name": "测试",
            "trackCount": 2,
            "tracks": [track, {"name": "缺少艺人"}]
        }
    });
    let error = parse_response(&malformed).expect_err("partially malformed response");
    assert!(error.to_string().contains("1/2"));
}

#[test]
fn skips_tracks_without_title_or_artists() {
    assert!(parse_track(&json!({ "name": "" })).is_none());
    assert!(parse_track(&json!({
        "name": "无艺人",
        "artists": [{"name": "  "}],
    }))
    .is_none());
    assert!(parse_track(&json!({})).is_none());
}
