use super::*;

#[tokio::test]
async fn remote_page_requires_exact_random_route_and_never_exposes_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = Arc::new(Mutex::new(State {
        revision: 7,
        stage: Stage::Scan,
        png: Some(vec![1, 2, 3]),
        task: None,
    }));
    let task = tokio::spawn(async move {
        for _ in 0..3 {
            let (stream, _) = listener.accept().await.unwrap();
            serve(
                stream,
                "/randomroute/",
                state.clone(),
                BilibiliClient::new(&crate::models::network::NetworkRoute::Direct).unwrap(),
                None,
            )
            .await
            .unwrap();
        }
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let root = client
        .get(format!("http://{address}/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(root.status(), 404);
    let status = client
        .get(format!("http://{address}/randomroute/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(status.headers()["cache-control"], "no-store");
    assert_eq!(status.headers()["referrer-policy"], "no-referrer");
    let body = status.text().await.unwrap();
    assert!(body.contains("has_qr"));
    for secret in ["SESSDATA", "cookie", "credential_key", "qrcode_key"] {
        assert!(!body.contains(secret));
    }
    let image = client
        .get(format!("http://{address}/randomroute/qr.png?v=7"))
        .send()
        .await
        .unwrap();
    assert_eq!(image.bytes().await.unwrap().as_ref(), &[1, 2, 3]);
    task.await.unwrap();
}

#[test]
fn refreshing_generation_rejects_old_publish_and_cancel_drops_qr() {
    let state = Arc::new(Mutex::new(State {
        revision: 2,
        stage: Stage::Scan,
        png: Some(vec![1]),
        task: None,
    }));
    assert!(!publish(&state, 1, Stage::Complete, None));
    assert!(publish(&state, 2, Stage::Confirm, None));
    assert!(state.lock().unwrap().png.is_some());
    cancel(&state);
    assert!(state.lock().unwrap().png.is_none());
    assert!(!publish(&state, 2, Stage::Scan, Some(vec![2])));
}
