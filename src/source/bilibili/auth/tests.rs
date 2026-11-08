use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
};

type RecordedRequests = Arc<Mutex<Vec<(String, BTreeMap<String, String>)>>>;

struct Fixture {
    origin: String,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    requests: RecordedRequests,
}

impl Fixture {
    fn new(mut respond: impl FnMut(&str) -> String + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let worker = thread::spawn(move || loop {
            let (stream, _) = listener.accept().unwrap();
            if stop.load(Ordering::SeqCst) {
                break;
            }
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let path = first.split_whitespace().nth(1).unwrap();
            let mut headers = BTreeMap::new();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                headers.insert(name.to_lowercase(), value.trim().to_string());
            }
            recorded.lock().unwrap().push((path.to_string(), headers));
            let _ = reader.get_mut().write_all(respond(path).as_bytes());
        });
        Self {
            origin,
            stopped,
            worker: Some(worker),
            requests,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.origin.strip_prefix("http://").unwrap());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn response(body: &str, headers: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len())
}
fn cookies() -> &'static str {
    "Set-Cookie: SESSDATA=fake-session; Domain=.bilibili.com; Path=/; Secure; HttpOnly\r\nSet-Cookie: bili_jct=fake-csrf; Domain=.bilibili.com; Path=/; Secure\r\nSet-Cookie: DedeUserID=123; Domain=.bilibili.com; Path=/; Secure\r\n"
}

#[tokio::test]
async fn qr_flow_verifies_identity_and_sends_cookies_only_to_account_requests() {
    let mut polls = 0;
    let fixture = Fixture::new(move |path| {
        if path.contains("/generate") {
            response(
                r#"{"code":0,"data":{"url":"https://account.bilibili.com/h5/account-h5/auth/scanweb?token=fake","qrcode_key":"fakekey123"}}"#,
                "",
            )
        } else if path.contains("/poll") {
            polls += 1;
            let code = [86101, 86090, 0][polls - 1];
            response(&format!("{{\"code\":0,\"data\":{{\"code\":{code},\"url\":\"secret-redirect\",\"refresh_token\":\"secret-token\"}}}}"), if code == 0 { cookies() } else { "" })
        } else if path.contains("/nav") {
            response(
                r#"{"code":0,"data":{"isLogin":true,"mid":123,"uname":"listener"}}"#,
                "",
            )
        } else {
            panic!("unexpected endpoint")
        }
    });
    let client = BilibiliClient::local(fixture.origin.clone());
    let qr = client.generate_qr().await.unwrap();
    let image = image::load_from_memory(&qr.png).unwrap().into_luma8();
    assert_eq!(image.get_pixel(0, 0).0, [255]);
    assert_eq!(format!("{qr:?}"), "QrCode([redacted])");
    assert!(matches!(
        client.poll_qr(&qr).await.unwrap(),
        PollResult::WaitingScan
    ));
    assert!(matches!(
        client.poll_qr(&qr).await.unwrap(),
        PollResult::WaitingConfirmation
    ));
    let PollResult::Confirmed(session) = client.poll_qr(&qr).await.unwrap() else {
        panic!("expected confirmation");
    };
    let account = client.verify_account(&session).await.unwrap();
    assert_eq!(account.id, 123);
    assert_eq!(account.name, "listener");
    assert_eq!(format!("{session:?}"), "AuthSession([redacted])");
    assert!(session.cookie_header().unwrap().is_sensitive());
    let restored = AuthSession::decode_secret(session.encode_secret(), 123).unwrap();
    assert_eq!(restored.account_id(), 123);
    assert_eq!(
        AuthSession::decode_secret(session.encode_secret(), 456).unwrap_err(),
        BilibiliError::AccountMismatch
    );
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    for (path, headers) in requests.iter() {
        assert_eq!(headers["referer"], super::super::client::REFERER);
        assert_eq!(headers.contains_key("cookie"), path.contains("/nav"));
    }
}

#[tokio::test]
async fn api_expiry_unknown_state_and_identity_mismatch_are_distinct() {
    let fixture = Fixture::new(|path| {
        if path.contains("/nav") {
            response(
                r#"{"code":0,"data":{"isLogin":true,"mid":456,"uname":"other"}}"#,
                "",
            )
        } else {
            response(r#"{"code":0,"data":{"code":86038}}"#, "")
        }
    });
    let client = BilibiliClient::local(fixture.origin.clone());
    let qr = QrCode {
        key: Zeroizing::new("fakekey".into()),
        png: Vec::new(),
    };
    assert!(matches!(
        client.poll_qr(&qr).await.unwrap(),
        PollResult::Expired
    ));
    let session = AuthSession::from_cookie(
        Zeroizing::new("SESSDATA=fake; bili_jct=csrf; DedeUserID=123".into()),
        None,
    )
    .unwrap();
    assert_eq!(
        client.verify_account(&session).await.unwrap_err(),
        BilibiliError::AccountMismatch
    );
    let fixture = Fixture::new(|_| response(r#"{"code":0,"data":{"code":99999}}"#, ""));
    assert!(matches!(
        BilibiliClient::local(fixture.origin.clone())
            .poll_qr(&qr)
            .await,
        Err(BilibiliError::Api(99999))
    ));
}

#[tokio::test]
async fn redirect_cannot_forward_session_and_errors_cannot_expose_body_or_qr_key() {
    let destination = Fixture::new(|_| panic!("redirect must not be followed"));
    let origin = destination.origin.clone();
    let fixture = Fixture::new(move |_| {
        format!("HTTP/1.1 302 Found\r\nLocation: {origin}/leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    });
    let session = AuthSession::from_cookie(
        Zeroizing::new("SESSDATA=fake; bili_jct=csrf; DedeUserID=123".into()),
        None,
    )
    .unwrap();
    assert_eq!(
        BilibiliClient::local(fixture.origin.clone())
            .verify_account(&session)
            .await
            .unwrap_err(),
        BilibiliError::Http(302)
    );
    assert!(destination.requests.lock().unwrap().is_empty());
    let fixture = Fixture::new(|_| response("secret-cookie-and-qr-key-invalid-json", ""));
    let qr = QrCode {
        key: Zeroizing::new("verysecretqrkey".into()),
        png: Vec::new(),
    };
    let error = BilibiliClient::local(fixture.origin.clone())
        .poll_qr(&qr)
        .await
        .unwrap_err();
    assert_eq!(error, BilibiliError::InvalidResponse);
    assert!(!format!("{error:?} {error}").contains("secret"));
}

#[tokio::test]
async fn invalid_qr_origin_and_oversize_responses_are_rejected() {
    let fixture = Fixture::new(|_| {
        response(
            r#"{"code":0,"data":{"url":"https://account.bilibili.com.evil.example/scan","qrcode_key":"fakekey"}}"#,
            "",
        )
    });
    assert_eq!(
        BilibiliClient::local(fixture.origin.clone())
            .generate_qr()
            .await
            .unwrap_err(),
        BilibiliError::InvalidResponse
    );
    let fixture = Fixture::new(|_| {
        "HTTP/1.1 200 OK\r\nContent-Length: 2097153\r\nConnection: close\r\n\r\n".into()
    });
    assert_eq!(
        BilibiliClient::local(fixture.origin.clone())
            .generate_qr()
            .await
            .unwrap_err(),
        BilibiliError::InvalidResponse
    );
}

#[test]
fn restored_cookie_cannot_inject_headers_and_proxy_cannot_embed_credentials() {
    for cookie in [
        "SESSDATA=fake\r\nAuthorization: secret; bili_jct=csrf; DedeUserID=123",
        "SESSDATA=fake; bili_jct=csrf; DedeUserID=123; unknown=secret",
        "SESSDATA=fake; SESSDATA=other; bili_jct=csrf; DedeUserID=123",
    ] {
        assert!(AuthSession::from_cookie(Zeroizing::new(cookie.into()), None).is_err());
    }
    for proxy in [
        "http://user:secret@127.0.0.1:7890",
        "file:///tmp/proxy",
        "http://127.0.0.1:7890?password=secret",
    ] {
        assert!(matches!(
            BilibiliClient::new(Some(proxy)),
            Err(BilibiliError::InvalidProxy)
        ));
    }
}
