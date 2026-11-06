use std::collections::HashMap;

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
fn accepts_urls_without_scheme() {
    let api = OpenSubsonicProvider::new("localhost:4533", "alice", "secret").unwrap();
    assert_eq!(api.base_url(), "http://localhost:4533");
}
