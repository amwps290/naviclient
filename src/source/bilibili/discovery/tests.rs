use super::super::tests::{video, Fixture};
use super::*;
use serde_json::json;

const BV: &str = "BV1GJ411x7h7";
const IMG: &str = "https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png";
const SUB: &str = "https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png";
fn nav() -> serde_json::Value {
    json!({"code":0,"data":{"isLogin":true,"mid":123,"wbi_img":{"img_url":IMG,"sub_url":SUB}}})
}
fn search_data() -> serde_json::Value {
    json!({"code":0,"data":{"page":1,"numPages":3,"numResults":60,"result":[{"bvid":BV,"title":"Live <em class=\"keyword\">音乐</em> &amp; More","author":"UP &amp; Name","pic":"//i0.hdslb.com/bfs/archive/a.jpg","duration":"01:03:04"}]}})
}

#[test]
fn ids_and_official_links_preserve_part_selection() {
    assert_eq!(
        parse_video_input(BV).unwrap().unwrap().id,
        Some(VideoId::Bvid(BV.into()))
    );
    assert_eq!(
        parse_video_input("AV42").unwrap().unwrap().id,
        Some(VideoId::Aid(42))
    );
    let link = parse_video_input(&format!(
        "https://www.bilibili.com/video/{BV}/?p=2&share_source=copy"
    ))
    .unwrap()
    .unwrap();
    assert_eq!(link.part, Some(1));
    assert_eq!(link.id, Some(VideoId::Bvid(BV.into())));
    assert!(parse_video_input("https://b23.tv/AbC123")
        .unwrap()
        .unwrap()
        .short
        .is_some());
    assert!(parse_video_input("音乐 live").unwrap().is_none());
    assert!(parse_video_input("Avatar soundtrack").unwrap().is_none());
}

#[test]
fn arbitrary_hosts_credentials_schemes_and_ambiguous_parts_are_not_links() {
    for value in [
        "BVbad",
        "av0",
        "https://evil.test/video/BV1GJ411x7h7",
        "https://www.bilibili.com.evil.test/video/BV1GJ411x7h7",
        "https://u:p@www.bilibili.com/video/BV1GJ411x7h7",
        "https://b23.tv:8443/abc",
        "file:///tmp/video",
        "https://www.bilibili.com/video/BV1GJ411x7h7?p=0",
        "https://www.bilibili.com/video/BV1GJ411x7h7?p=2&p=3",
    ] {
        assert!(
            parse_video_input(value).is_err(),
            "accepted unsafe input: {value}"
        );
    }
}

#[test]
fn wbi_signature_matches_independent_public_example_and_rejects_bad_keys() {
    let signed = wbi::sign(
        IMG,
        SUB,
        &[
            ("foo", "114".into()),
            ("bar", "514".into()),
            ("baz", "1919810".into()),
        ],
        1702204169,
    )
    .unwrap();
    let params = signed
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(params["w_rid"], "6149fdadf571698ca7e6a567265cd0ee");
    assert_eq!(params["wts"], "1702204169");
    assert!(wbi::sign("https://evil.test/key.png", SUB, &[], 1).is_err());
    assert_eq!(
        plain_title("A <em>B</em> &quot;C&quot; &amp; D"),
        "A B \"C\" & D"
    );
    assert_eq!(duration("4:35"), Some(275));
    assert!(duration("1:60").is_none());
}

#[tokio::test]
async fn search_is_signed_authenticated_bounded_metadata_only() {
    let fixture = Fixture::new(|path| {
        if path.starts_with("/x/web-interface/nav") {
            nav()
        } else if path.starts_with("/x/web-interface/wbi/search/type") {
            search_data()
        } else {
            panic!("unexpected discovery request {path}")
        }
    });
    let result = fixture.provider().search_videos("音乐", 1).await.unwrap();
    assert_eq!(result.page, 1);
    assert_eq!(result.pages, 3);
    assert_eq!(result.total, 60);
    assert_eq!(result.videos[0].title, "Live 音乐 & More");
    assert_eq!(result.videos[0].uploader, "UP & Name");
    assert_eq!(result.videos[0].duration, 3784);
    assert!(result.videos[0]
        .cover
        .as_ref()
        .unwrap()
        .starts_with("https://i0.hdslb.com/"));
    let paths = fixture.paths.lock().unwrap();
    assert_eq!(paths.len(), 2);
    let url = url::Url::parse(&format!("http://fixture{}", paths[1])).unwrap();
    let params = url
        .query_pairs()
        .into_owned()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(params["keyword"], "音乐");
    assert_eq!(params["search_type"], "video");
    assert_eq!(params["page_size"], "20");
    assert_eq!(params["w_rid"].len(), 32);
    assert!(fixture
        .headers
        .lock()
        .unwrap()
        .iter()
        .all(|headers| headers.contains_key("cookie")));
}

#[tokio::test]
async fn login_mismatch_and_rate_limit_stop_without_fallback_requests() {
    for (login, mid, code, expected_count, expected) in [
        (false, 0, 0, 1, BilibiliError::NotAuthenticated),
        (true, 124, 0, 1, BilibiliError::AccountMismatch),
        (true, 123, -412, 2, BilibiliError::RateLimited),
    ] {
        let fixture = Fixture::new(move |path| {
            if path.starts_with("/x/web-interface/nav") {
                let mut data = nav();
                data["data"]["isLogin"] = json!(login);
                data["data"]["mid"] = json!(mid);
                data
            } else {
                json!({"code":code,"data":null})
            }
        });
        assert_eq!(
            fixture
                .provider()
                .search_videos("music", 1)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(fixture.paths.lock().unwrap().len(), expected_count);
    }
}

#[tokio::test]
async fn public_av_and_bv_link_open_parts_without_requesting_favorites_or_audio() {
    let fixture = Fixture::new(|path| {
        assert!(path.starts_with("/x/web-interface/view"));
        let mut data = video(BV, &[100, 101]);
        data["data"]["aid"] = json!(42);
        data
    });
    let provider = fixture.provider();
    let av = provider.open_video("av42").await.unwrap();
    assert_eq!(av.songs.len(), 2);
    assert_eq!(av.songs[1].id, format!("{BV}:101"));
    assert!(av
        .songs
        .iter()
        .all(|song| song.source.as_ref() == Some(provider.id())));
    let selected = provider
        .open_video(&format!("https://www.bilibili.com/video/{BV}?p=2"))
        .await
        .unwrap();
    assert_eq!(selected.selected, 1);
    assert_eq!(fixture.paths.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn short_links_receive_no_cookies_and_external_redirects_are_not_followed() {
    for safe in [true, false] {
        let fixture = Fixture::with_response(move |path, headers| {
            if path == "/AbC123" {
                assert!(!headers.contains_key("cookie"));
                assert!(!headers.contains_key("authorization"));
                let target = if safe {
                    format!("https://www.bilibili.com/video/{BV}?p=2")
                } else {
                    "https://evil.test/leak".into()
                };
                format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
            } else {
                assert!(safe && path.starts_with("/x/web-interface/view"));
                assert!(headers.contains_key("cookie"));
                let body = video(BV, &[100, 101]).to_string();
                format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
            }
        });
        let result = fixture.provider().open_video("https://b23.tv/AbC123").await;
        if safe {
            assert_eq!(result.unwrap().selected, 1);
        } else {
            assert!(result.is_err());
        }
        assert_eq!(
            fixture.paths.lock().unwrap().len(),
            if safe { 2 } else { 1 }
        );
    }
}

#[tokio::test]
async fn empty_search_and_malformed_metadata_have_distinct_results() {
    for invalid in [false, true] {
        let fixture = Fixture::new(move |path| {
            if path.starts_with("/x/web-interface/nav") {
                return nav();
            }
            if invalid {
                let mut data = search_data();
                data["data"]["result"][0]["duration"] = json!("1:60");
                data
            } else {
                json!({"code":0,"data":{"page":1,"numPages":0,"numResults":0,"result":[]}})
            }
        });
        let result = fixture.provider().search_videos("music", 1).await;
        if invalid {
            assert_eq!(result.unwrap_err(), BilibiliError::InvalidResponse);
        } else {
            let result = result.unwrap();
            assert!(result.videos.is_empty());
            assert_eq!(result.pages, 1);
        }
        assert_eq!(fixture.paths.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn http_risk_controls_stop_search_without_reading_or_replaying_error_bodies() {
    for status in [412, 429] {
        let fixture = Fixture::with_response(move |path, _| {
            if path.starts_with("/x/web-interface/nav") {
                let body = nav().to_string();
                format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
            } else {
                format!(
                    "HTTP/1.1 {status} Blocked\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .into_bytes()
            }
        });
        assert_eq!(
            fixture
                .provider()
                .search_videos("music", 1)
                .await
                .unwrap_err(),
            BilibiliError::RateLimited
        );
        assert_eq!(fixture.paths.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn invalid_input_and_pages_never_issue_network_requests() {
    let fixture = Fixture::new(|_| panic!("invalid input reached the network"));
    let provider = fixture.provider();
    assert!(provider.search_videos("music", 0).await.is_err());
    assert!(provider.search_videos("music", 101).await.is_err());
    assert!(provider
        .open_video("https://127.0.0.1/video/BV1GJ411x7h7")
        .await
        .is_err());
    assert!(fixture.paths.lock().unwrap().is_empty());
}

#[tokio::test]
async fn mismatched_video_identity_and_invalid_part_do_not_start_media() {
    let fixture = Fixture::new(|path| {
        assert!(path.starts_with("/x/web-interface/view"));
        let mut data = video(BV, &[100]);
        data["data"]["aid"] = json!(43);
        data
    });
    let provider = fixture.provider();
    assert_eq!(
        provider.open_video("av42").await.unwrap_err(),
        BilibiliError::InvalidResponse
    );
    assert_eq!(
        provider
            .open_video(&format!("https://www.bilibili.com/video/{BV}?p=2"))
            .await
            .unwrap_err(),
        BilibiliError::InvalidVideoInput
    );
    assert_eq!(fixture.paths.lock().unwrap().len(), 2);
}
