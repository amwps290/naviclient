//! Exercise API, cover, and streaming routing against a local HTTP proxy.

use super::*;
use crate::test_support::http::Server;

#[tokio::test]
async fn subsonic_api_cover_and_audio_all_use_the_selected_proxy() {
    let server = Server::new(vec![
        (
            200,
            r#"{"subsonic-response":{"status":"ok","searchResult3":{}}}"#.into(),
        ),
        (200, "cover-fixture".into()),
        (200, "audio-fixture".into()),
    ]);
    let proxy = server.url("");
    // This domain cannot be reached directly; the fixture proxy answers every request.
    let route = NetworkRoute::Proxy(proxy.trim_end_matches('/').into());
    let source = MusicSource::opensubsonic_with_route(
        "test-source",
        "http://music.invalid",
        "listener",
        "password",
        route.clone(),
    )
    .unwrap();
    source.search("music", 10, 10, 10).await.unwrap();
    let cover = source.cover_url("cover", 384).unwrap();
    assert_eq!(source.get_bytes(&cover).await.unwrap(), b"cover-fixture");
    let song = crate::models::Song {
        id: "track".into(),
        ..Default::default()
    };
    let resource = source
        .resolve_playback(&song, crate::models::TranscodingQuality::Original)
        .await
        .unwrap();
    assert_eq!(resource.http.route(), &route);
    let bytes = tokio::task::spawn_blocking(move || {
        let client = resource::clients::blocking(&resource.http).unwrap();
        client
            .get(resource.http.url())
            .send()
            .unwrap()
            .bytes()
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(bytes.as_ref(), b"audio-fixture");
    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    for (request, endpoint) in requests.iter().zip(["search3", "getCoverArt", "stream"]) {
        assert_eq!(request.url.host_str(), Some("music.invalid"));
        assert_eq!(request.url.path(), format!("/rest/{endpoint}.view"));
        assert!(!request.url.as_str().contains("password"));
    }
}
