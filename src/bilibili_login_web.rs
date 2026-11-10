//! Explicit remote login mode; never started by ordinary application startup.
//! The random route exposes only the QR image and sanitized progress.

use crate::{
    config,
    secrets::source_credentials::{
        CredentialError, CredentialStore, StagedCredential, SystemStore,
    },
    source::bilibili::{
        account,
        auth::{AuthSession, PollResult, POLL_INTERVAL, QR_LIFETIME},
        client::{BilibiliClient, BilibiliError},
    },
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::AbortHandle,
};

const PAGE: &str = r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>B 站登录</title>
<style>body{margin:0;background:#f7f5f1;color:#25231f;font-family:system-ui,sans-serif}
main{max-width:520px;margin:6vh auto;padding:28px;text-align:center}h1{font-size:25px}
.card{background:white;border:1px solid #e2ddd5;border-radius:18px;padding:22px}
img{display:block;width:min(100%,300px);height:auto;margin:auto}p{line-height:1.7}
button{border:0;border-radius:24px;background:#645548;color:white;padding:11px 22px;font-size:15px;cursor:pointer}
button:disabled{opacity:.5}</style><main><h1>B 站扫码登录</h1>
<p>使用 B 站 App 扫码，并在手机上确认登录。</p><div class="card">
<img id="qr" alt="登录二维码" hidden><p id="status" role="status">正在生成二维码…</p>
<button id="refresh">重新生成二维码</button> <button id="cancel">取消</button></div>
<p>登录凭据保存在远程主机的系统凭据库中。</p></main><script>
let revision=null, ended=false;
async function update(){if(ended)return;try{let r=await fetch('status',{cache:'no-store'}),s=await r.json();
document.getElementById('status').textContent=s.message;let qr=document.getElementById('qr');
if(s.has_qr && revision!==s.revision){revision=s.revision;qr.src='qr.png?v='+revision;}qr.hidden=!s.has_qr;
document.getElementById('refresh').disabled=s.complete;document.getElementById('cancel').disabled=s.complete;
}catch(e){document.getElementById('status').textContent='登录网页已关闭。';ended=true;}}
document.getElementById('refresh').onclick=async()=>{await fetch('refresh',{method:'POST'});await update();};
document.getElementById('cancel').onclick=async()=>{await fetch('cancel',{method:'POST'});await update();};
update();setInterval(update,1000);</script></html>"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Generating,
    Scan,
    Confirm,
    Verify,
    Save,
    Complete,
    Expired,
    Failed(&'static str),
    Cancelled,
}
impl Stage {
    fn message(self) -> &'static str {
        match self {
            Self::Generating => "正在检查凭据库并生成二维码…",
            Self::Scan => "请使用 B 站 App 扫码。",
            Self::Confirm => "已扫码，请在 B 站 App 中确认登录。",
            Self::Verify => "正在核对登录账号…",
            Self::Save => "正在保存并回读验证系统凭据…",
            Self::Complete => "登录成功，账号已安全保存。重新启动播放器后生效。",
            Self::Expired => "二维码已过期，请重新生成。",
            Self::Failed(message) => message,
            Self::Cancelled => "登录已取消。",
        }
    }
}

struct State {
    revision: u128,
    stage: Stage,
    png: Option<Vec<u8>>,
    task: Option<AbortHandle>,
}
type SharedState = Arc<Mutex<State>>;

fn publish(state: &SharedState, revision: u128, stage: Stage, png: Option<Vec<u8>>) -> bool {
    let mut state = state.lock().unwrap();
    if state.revision != revision || state.stage == Stage::Cancelled {
        return false;
    }
    state.stage = stage;
    if matches!(stage, Stage::Scan | Stage::Confirm) {
        if png.is_some() {
            state.png = png;
        }
    } else {
        state.png = None;
    }
    true
}

fn credential_message(error: CredentialError) -> &'static str {
    match error {
        CredentialError::Locked => "请解锁远程主机的系统凭据库，再重新生成二维码。",
        CredentialError::Missing | CredentialError::Invalid => "系统凭据验证失败，请重试。",
        CredentialError::Unavailable => "远程主机的系统凭据库不可用，无法保存登录。",
    }
}
fn protocol_message(error: BilibiliError) -> &'static str {
    match error {
        BilibiliError::InvalidProxy => {
            "请输入有效的代理地址（HTTP、HTTPS 或 SOCKS5），或省略 --proxy。"
        }
        BilibiliError::Network | BilibiliError::Http(_) => {
            "无法连接 B 站，请检查网络或代理后重试。"
        }
        _ => "B 站登录验证失败，请重新生成二维码。",
    }
}

fn restart(state: &SharedState, client: BilibiliClient, proxy: Option<String>) {
    let revision = rand::random();
    {
        let mut current = state.lock().unwrap();
        if current.stage == Stage::Complete {
            return;
        }
        if let Some(task) = current.task.take() {
            task.abort();
        }
        current.revision = revision;
        current.stage = Stage::Generating;
        current.png = None;
    }
    let shared = state.clone();
    let task = tokio::spawn(async move {
        let result: Result<(), &'static str> = async {
            SystemStore.check(true).await.map_err(credential_message)?;
            let qr = client.generate_qr().await.map_err(protocol_message)?;
            if !publish(&shared, revision, Stage::Scan, Some(qr.png.clone())) {
                return Ok(());
            }
            let deadline = tokio::time::Instant::now() + QR_LIFETIME;
            let session = loop {
                match tokio::time::timeout_at(deadline, client.poll_qr(&qr)).await {
                    Err(_) | Ok(Ok(PollResult::Expired)) => {
                        publish(&shared, revision, Stage::Expired, None);
                        return Ok(());
                    }
                    Ok(Err(error)) => return Err(protocol_message(error)),
                    Ok(Ok(PollResult::WaitingScan)) => {}
                    Ok(Ok(PollResult::WaitingConfirmation)) => {
                        if !publish(&shared, revision, Stage::Confirm, None) {
                            return Ok(());
                        }
                    }
                    Ok(Ok(PollResult::Confirmed(session))) => break session,
                }
                tokio::time::sleep_until(
                    (tokio::time::Instant::now() + POLL_INTERVAL).min(deadline),
                )
                .await;
            };
            if !publish(&shared, revision, Stage::Verify, None) {
                return Ok(());
            }
            let verified = client
                .verify_account(&session)
                .await
                .map_err(protocol_message)?;
            let source = account::source_for_account(&config::load(), verified.id);
            if !publish(&shared, revision, Stage::Save, None) {
                return Ok(());
            }
            let staged = StagedCredential::create(
                Arc::new(SystemStore),
                &source,
                verified.id,
                session.encode_secret(),
            )
            .await
            .map_err(credential_message)?;
            let restored = AuthSession::decode_secret(
                SystemStore
                    .load(staged.key())
                    .await
                    .map_err(credential_message)?,
                verified.id,
            )
            .map_err(protocol_message)?;
            client
                .verify_account(&restored)
                .await
                .map_err(protocol_message)?;
            let old_key = {
                let mut current = shared.lock().unwrap();
                if current.revision != revision || current.stage == Stage::Cancelled {
                    return Ok(());
                }
                let (next, old_key) = account::prepare_config(
                    &config::load(),
                    &source,
                    &verified,
                    &restored,
                    staged.key(),
                    proxy,
                )
                .map_err(credential_message)?;
                config::save(&next).map_err(|_| "无法保存账号设置，请重试。")?;
                staged.commit();
                current.stage = Stage::Complete;
                current.png = None;
                old_key
            };
            if let Some(key) = old_key {
                if crate::secrets::source_credentials::validate_reference(
                    &source,
                    verified.id,
                    &key,
                )
                .is_ok()
                {
                    let _ = SystemStore.delete(&key).await;
                }
            }
            Ok(())
        }
        .await;
        if let Err(message) = result {
            publish(&shared, revision, Stage::Failed(message), None);
        }
    });
    state.lock().unwrap().task = Some(task.abort_handle());
}

fn cancel(state: &SharedState) {
    let mut current = state.lock().unwrap();
    if current.stage != Stage::Complete {
        current.stage = Stage::Cancelled;
        current.png = None;
        if let Some(task) = current.task.take() {
            task.abort();
        }
    }
}

async fn serve(
    mut stream: TcpStream,
    root: &str,
    state: SharedState,
    client: BilibiliClient,
    proxy: Option<String>,
) -> std::io::Result<()> {
    let mut header = Vec::new();
    let mut buffer = [0u8; 1024];
    while !header.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await?;
        if count == 0 || header.len() + count > 8192 {
            return Ok(());
        }
        header.extend_from_slice(&buffer[..count]);
    }
    let text = std::str::from_utf8(&header).unwrap_or("");
    let mut first = text.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("");
    let path = first.next().unwrap_or("").split('?').next().unwrap_or("");
    let route = path.strip_prefix(root);
    let (status, mime, body) = match (method, route) {
        ("GET", Some("")) => (200, "text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
        ("GET", Some("status")) => {
            let current = state.lock().unwrap();
            let body = serde_json::json!({"revision":format!("{:032x}", current.revision), "message": current.stage.message(), "has_qr": current.png.is_some(), "complete": current.stage == Stage::Complete});
            (200, "application/json", body.to_string().into_bytes())
        }
        ("GET", Some("qr.png")) => match state.lock().unwrap().png.clone() {
            Some(png) => (200, "image/png", png),
            None => (404, "text/plain", Vec::new()),
        },
        ("POST", Some("refresh")) => {
            restart(&state, client, proxy);
            (204, "text/plain", Vec::new())
        }
        ("POST", Some("cancel")) => {
            cancel(&state);
            (204, "text/plain", Vec::new())
        }
        _ => (404, "text/plain", Vec::new()),
    };
    let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&body).await
}

/// Runs before file logging and GUI initialization; no login token enters logs.
pub(crate) fn try_run() -> bool {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--bilibili-login-web") {
        return false;
    }
    let mut port = 8765;
    let mut proxy = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => match args
                .next()
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|port| *port > 0)
            {
                Some(value) => port = value,
                None => {
                    eprintln!("端口无效。");
                    return true;
                }
            },
            "--proxy" => match args.next() {
                Some(value) => proxy = Some(value),
                None => {
                    eprintln!("--proxy 缺少地址。");
                    return true;
                }
            },
            _ => {
                eprintln!("用法：--bilibili-login-web [--port 8765] [--proxy http://HOST:PORT]");
                return true;
            }
        }
    }
    let runtime = tokio::runtime::Runtime::new().expect("failed to start login runtime");
    if let Err(error) = runtime.block_on(run(port, proxy)) {
        eprintln!("{error}");
    }
    true
}

async fn run(port: u16, proxy: Option<String>) -> Result<(), &'static str> {
    let proxy = proxy
        .map(|proxy| crate::models::network::normalize_proxy(&proxy))
        .transpose()
        .map_err(|_| protocol_message(BilibiliError::InvalidProxy))?;
    let route = proxy
        .clone()
        .map_or(crate::models::network::NetworkRoute::System, |proxy| {
            crate::models::network::NetworkRoute::Proxy(proxy)
        });
    let client = BilibiliClient::new(&route).map_err(protocol_message)?;
    SystemStore.check(false).await.map_err(credential_message)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
        .await
        .map_err(|_| "无法监听登录端口，请使用 --port 指定其他端口。")?;
    let root = format!("/{:032x}/", rand::random::<u128>());
    println!("B 站登录监听 0.0.0.0:{port}，关闭播放器后使用本入口。\n请打开 http://服务器IP:{port}{root}");
    let state = Arc::new(Mutex::new(State {
        revision: 0,
        stage: Stage::Generating,
        png: None,
        task: None,
    }));
    restart(&state, client.clone(), proxy.clone());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
    let slots = Arc::new(tokio::sync::Semaphore::new(4));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep_until(deadline) => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|_| "登录网页服务中断。")?;
                if let Ok(permit) = slots.clone().try_acquire_owned() {
                    let (state, root, client, proxy) = (state.clone(), root.clone(), client.clone(), proxy.clone());
                    tokio::spawn(async move { let _permit = permit; let _ = tokio::time::timeout(Duration::from_secs(5), serve(stream, &root, state, client, proxy)).await; });
                }
            }
        }
    }
    cancel(&state);
    let _ = tokio::time::timeout(
        Duration::from_secs(30),
        crate::secrets::source_credentials::settle_staging(),
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests;
