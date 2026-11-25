use super::*;
use crate::{
    models::network::NetworkRoute,
    source::ProviderKind,
    test_support::http::{Request, Server},
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

const BV: &str = "BV1GJ411x7h7";
fn provider(server: &Server) -> BilibiliProvider {
    BilibiliProvider {
        id: SourceId::new(ProviderKind::Bilibili, "test"),
        client: BilibiliClient::local(server.url("")),
        session: super::super::tests::session(),
        route: NetworkRoute::Direct,
    }
}
fn respond(request: &Request, present: bool) -> serde_json::Value {
    match request.url.path() {
        "/x/web-interface/nav" => {
            json!({"code":0,"data":{"isLogin":true,"mid":123,"uname":"User"}})
        }
        "/x/web-interface/view" => {
            let mut data = super::super::tests::video(BV, &[10, 11]);
            data["data"]["aid"] = json!(42);
            data
        }
        "/x/v3/fav/folder/info" => {
            json!({"code":0,"data":{"id":5,"title":"Music","media_count":3,"cover":null,"upper":{"mid":123}}})
        }
        "/x/v3/fav/folder/created/list-all" => {
            json!({"code":0,"data":{"list":[{"id":5,"mid":123,"title":"Music","fav_state":u8::from(present)},{"id":6,"mid":123,"title":"Other","fav_state":0}]}})
        }
        _ => panic!("unexpected request {}", request.url.path()),
    }
}

#[tokio::test]
async fn add_and_remove_use_video_aid_owned_folder_and_csrf_then_verify() {
    for (initial, desired) in [(false, true), (true, false)] {
        let present = Arc::new(AtomicBool::new(initial));
        let writes = Arc::new(AtomicUsize::new(0));
        let observed = present.clone();
        let count = writes.clone();
        let server = Server::with_handler(move |request| {
            assert!(request
                .headers
                .get("cookie")
                .unwrap()
                .contains("SESSDATA=fake"));
            if request.method == "POST" {
                assert_eq!(request.url.path(), "/x/v3/fav/resource/deal");
                let form = url::form_urlencoded::parse(&request.body)
                    .into_owned()
                    .collect::<std::collections::BTreeMap<_, _>>();
                assert_eq!(form["rid"], "42");
                assert_eq!(form["type"], "2");
                assert_eq!(form["csrf"], "csrf");
                assert_eq!(form["add_media_ids"], if desired { "5" } else { "" });
                assert_eq!(form["del_media_ids"], if desired { "" } else { "5" });
                observed.store(desired, Ordering::SeqCst);
                count.fetch_add(1, Ordering::SeqCst);
                return (200, json!({"code":0,"data":null}).to_string());
            }
            (
                200,
                respond(request, observed.load(Ordering::SeqCst)).to_string(),
            )
        });
        let provider = provider(&server);
        let snapshot = provider.prepare_video_favorite(BV).await.unwrap();
        assert_eq!(snapshot.membership(5), Some(initial));
        let result = provider
            .change_video_favorite(
                &snapshot,
                FavoriteOperation {
                    folder_id: 5,
                    present: desired,
                },
            )
            .await
            .unwrap();
        let FavoriteOutcome::Verified(result) = result else {
            panic!("change was not verified")
        };
        assert_eq!(result.membership(5), Some(desired));
        assert_eq!(result.membership(6), Some(false));
        assert_eq!(writes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn target_ownership_is_rechecked_independently_before_any_post() {
    let server = Server::with_handler(|request| {
        assert_eq!(request.method, "GET");
        if request.url.path() == "/x/v3/fav/folder/info" {
            return (200, json!({"code":0,"data":{"id":5,"title":"Foreign","cover":null,"upper":{"mid":124}}}).to_string());
        }
        (200, respond(request, false).to_string())
    });
    let provider = provider(&server);
    let snapshot = provider.prepare_video_favorite(BV).await.unwrap();
    assert_eq!(
        provider
            .change_video_favorite(
                &snapshot,
                FavoriteOperation {
                    folder_id: 5,
                    present: true
                }
            )
            .await
            .unwrap_err(),
        BilibiliError::PermissionDenied
    );
    assert!(server
        .requests()
        .iter()
        .all(|request| request.method == "GET"));
}

#[tokio::test]
async fn missing_membership_flags_are_not_treated_as_permission_to_write() {
    let server = Server::with_handler(|request| {
        assert_eq!(request.method, "GET");
        let mut value = respond(request, false);
        if request.url.path() == "/x/v3/fav/folder/created/list-all" {
            value["data"]["list"][0]
                .as_object_mut()
                .unwrap()
                .remove("fav_state");
        }
        (200, value.to_string())
    });
    let provider = provider(&server);
    let snapshot = provider.prepare_video_favorite(BV).await.unwrap();
    assert_eq!(snapshot.membership(5), None);
    assert!(provider
        .change_video_favorite(
            &snapshot,
            FavoriteOperation {
                folder_id: 5,
                present: true
            }
        )
        .await
        .is_err());
}

#[tokio::test]
async fn already_desired_state_never_sends_a_post() {
    let server = Server::with_handler(|request| {
        assert_eq!(request.method, "GET");
        (200, respond(request, true).to_string())
    });
    let provider = provider(&server);
    let snapshot = provider.prepare_video_favorite(BV).await.unwrap();
    assert!(matches!(
        provider
            .change_video_favorite(
                &snapshot,
                FavoriteOperation {
                    folder_id: 5,
                    present: true
                }
            )
            .await
            .unwrap(),
        FavoriteOutcome::Verified(_)
    ));
    assert!(server
        .requests()
        .iter()
        .all(|request| request.method == "GET"));
}

#[tokio::test]
async fn a_lost_ack_is_verified_without_reposting_and_unknown_result_stays_uncertain() {
    for committed in [false, true] {
        let present = Arc::new(AtomicBool::new(false));
        let observed = present.clone();
        let server = Server::with_handler(move |request| {
            if request.method == "POST" {
                observed.store(committed, Ordering::SeqCst);
                return (502, "proxy lost response".into());
            }
            (
                200,
                respond(request, observed.load(Ordering::SeqCst)).to_string(),
            )
        });
        let provider = provider(&server);
        let snapshot = provider.prepare_video_favorite(BV).await.unwrap();
        let result = provider
            .change_video_favorite(
                &snapshot,
                FavoriteOperation {
                    folder_id: 5,
                    present: true,
                },
            )
            .await
            .unwrap();
        if committed {
            assert!(matches!(result, FavoriteOutcome::Verified(_)));
        } else {
            let FavoriteOutcome::Uncertain { snapshot, .. } = result else {
                panic!("lost response falsely reported success")
            };
            assert_eq!(snapshot.membership(5), None);
        }
        assert_eq!(
            server
                .requests()
                .iter()
                .filter(|request| request.method == "POST")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn forged_source_account_and_unknown_folder_cannot_write() {
    let server = Server::with_handler(|request| {
        assert_eq!(request.method, "GET");
        (200, respond(request, false).to_string())
    });
    let provider = provider(&server);
    let snapshot = provider.prepare_video_favorite(BV).await.unwrap();
    let reads = server.requests().len();
    let mut wrong = snapshot.clone();
    wrong.account_id = 124;
    assert_eq!(
        provider
            .change_video_favorite(
                &wrong,
                FavoriteOperation {
                    folder_id: 5,
                    present: true
                }
            )
            .await
            .unwrap_err(),
        BilibiliError::AccountMismatch
    );
    wrong = snapshot.clone();
    wrong.source = SourceId::new(ProviderKind::Bilibili, "other");
    assert_eq!(
        provider
            .change_video_favorite(
                &wrong,
                FavoriteOperation {
                    folder_id: 5,
                    present: true
                }
            )
            .await
            .unwrap_err(),
        BilibiliError::AccountMismatch
    );
    assert_eq!(server.requests().len(), reads);
    assert!(provider
        .change_video_favorite(
            &snapshot,
            FavoriteOperation {
                folder_id: 99,
                present: true
            }
        )
        .await
        .is_err());
    assert!(server
        .requests()
        .iter()
        .all(|request| request.method == "GET"));
}

#[tokio::test]
async fn rate_limit_is_uncertain_and_does_not_retry_or_fetch_after_the_post() {
    let server = Server::with_handler(|request| {
        if request.method == "POST" {
            (429, "rate limited".into())
        } else {
            (200, respond(request, false).to_string())
        }
    });
    let provider = provider(&server);
    let snapshot = provider.prepare_video_favorite(BV).await.unwrap();
    let result = provider
        .change_video_favorite(
            &snapshot,
            FavoriteOperation {
                folder_id: 5,
                present: true,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        result,
        FavoriteOutcome::Uncertain {
            error: BilibiliError::RateLimited,
            ..
        }
    ));
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        1
    );
    assert_eq!(requests.last().unwrap().method, "POST");
}
