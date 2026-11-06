use std::collections::HashMap;

use super::*;
use url::Url;

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
