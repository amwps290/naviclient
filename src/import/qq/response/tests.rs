use super::*;
use serde_json::json;

#[test]
fn parses_qq_playlist_and_converts_seconds_to_milliseconds() {
    let body = json!({
        "code": 0,
        "req": {
            "code": 0,
            "data": {
                "code": 0,
                "subcode": 0,
                "dirinfo": {"title": "通勤歌单", "songnum": 2},
                "songlist": [
                    {
                        "name": "爱人错过",
                        "singer": [{"name": "告五人"}],
                        "album": {"name": "我肯定在几百年前就说过爱你"},
                        "interval": 292
                    },
                    {
                        "title": "漠河舞厅",
                        "singer": [{"name": "柳爽"}, {"name": ""}],
                        "album": {"name": "1st.星球"},
                        "interval": "333"
                    }
                ]
            }
        }
    });
    let playlist = parse_response(&body).expect("valid QQ playlist");
    assert_eq!(playlist.name, "通勤歌单");
    assert_eq!(playlist.tracks.len(), 2);
    assert_eq!(playlist.tracks[0].artists, vec!["告五人"]);
    assert_eq!(playlist.tracks[0].duration_ms, 292_000);
    assert_eq!(playlist.tracks[1].duration_ms, 333_000);
    assert_eq!(playlist.unavailable_tracks, 0);
}

#[test]
fn accounts_for_unavailable_tracks_and_rejects_malformed_rows() {
    let unavailable = json!({
        "code": 0,
        "req": {"code": 0, "data": {
            "code": 0,
            "subcode": 0,
            "dirinfo": {"title": "测试", "songnum": 2},
            "songlist": [{
                "name": "晴天",
                "singer": [{"name": "周杰伦"}],
                "interval": 269
            }]
        }}
    });
    let playlist = parse_response(&unavailable).expect("one known unavailable track");
    assert_eq!(playlist.tracks.len(), 1);
    assert_eq!(playlist.unavailable_tracks, 1);

    let malformed = json!({
        "code": 0,
        "req": {"code": 0, "data": {
            "code": 0,
            "subcode": 0,
            "dirinfo": {"title": "测试", "songnum": 2},
            "songlist": [
                {
                    "name": "晴天",
                    "singer": [{"name": "周杰伦"}],
                    "interval": 269
                },
                {"name": "缺少歌手"}
            ]
        }}
    });
    let error = parse_response(&malformed).expect_err("malformed row must not be hidden");
    assert!(error.to_string().contains("1/2"));
}

#[test]
fn rejects_private_empty_and_error_responses() {
    assert!(parse_response(&json!({
        "code": 0,
        "req": {"code": 0, "data": {
            "code": 0,
            "subcode": 0,
            "dirinfo": {"title": "空", "songnum": 0},
            "songlist": []
        }}
    }))
    .is_err());
    assert!(parse_response(&json!({"code": 4000})).is_err());
    assert!(parse_response(&json!({
        "code": 0,
        "req": {"code": 0, "data": {"code": 0, "subcode": 4000}}
    }))
    .is_err());
}
