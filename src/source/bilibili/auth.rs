//! QR login and verified cookie sessions. Secrets have no ordinary serde/Debug.

use super::client::{BilibiliClient, BilibiliError};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::{collections::BTreeMap, fmt, io::Cursor, sync::Arc, time::Duration};
use zeroize::Zeroizing;

const COOKIE_NAMES: [&str; 5] = [
    "SESSDATA",
    "bili_jct",
    "DedeUserID",
    "DedeUserID__ckMd5",
    "sid",
];
pub const QR_LIFETIME: Duration = Duration::from_secs(180);
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: u64,
    pub name: String,
}

#[derive(Clone)]
pub struct AuthSession {
    account_id: u64,
    cookie: Arc<Zeroizing<String>>,
}

impl fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthSession([redacted])")
    }
}

impl AuthSession {
    fn from_headers(headers: &HeaderMap) -> Result<Self, BilibiliError> {
        let mut cookies = BTreeMap::new();
        for value in headers.get_all(header::SET_COOKIE) {
            let raw = value.to_str().map_err(|_| BilibiliError::InvalidResponse)?;
            let mut attributes = raw.split(';');
            let (name, value) = attributes
                .next()
                .and_then(|pair| pair.trim().split_once('='))
                .ok_or(BilibiliError::InvalidResponse)?;
            if !COOKIE_NAMES.contains(&name) {
                continue;
            }
            let mut valid_domain = false;
            let mut valid_path = false;
            for attribute in attributes {
                if let Some((key, value)) = attribute.trim().split_once('=') {
                    if key.eq_ignore_ascii_case("domain") {
                        valid_domain = value
                            .trim_start_matches('.')
                            .eq_ignore_ascii_case("bilibili.com");
                    } else if key.eq_ignore_ascii_case("path") {
                        valid_path = value == "/";
                    }
                }
            }
            if !valid_domain || !valid_path || !valid_cookie_value(value) {
                return Err(BilibiliError::InvalidResponse);
            }
            cookies.insert(name, value);
        }
        let cookie = cookies
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        Self::from_cookie(Zeroizing::new(cookie), None)
    }

    fn from_cookie(
        cookie: Zeroizing<String>,
        expected: Option<u64>,
    ) -> Result<Self, BilibiliError> {
        if cookie.len() > 4096 {
            return Err(BilibiliError::InvalidResponse);
        }
        let mut values = BTreeMap::new();
        for pair in cookie.split("; ") {
            let (name, value) = pair.split_once('=').ok_or(BilibiliError::InvalidResponse)?;
            if !COOKIE_NAMES.contains(&name)
                || !valid_cookie_value(value)
                || values.insert(name, value).is_some()
            {
                return Err(BilibiliError::InvalidResponse);
            }
        }
        if !values.contains_key("SESSDATA") || !values.contains_key("bili_jct") {
            return Err(BilibiliError::NotAuthenticated);
        }
        let account_id = values
            .get("DedeUserID")
            .and_then(|id| id.parse::<u64>().ok())
            .filter(|id| *id > 0)
            .ok_or(BilibiliError::NotAuthenticated)?;
        if expected.is_some_and(|expected| expected != account_id) {
            return Err(BilibiliError::AccountMismatch);
        }
        Ok(Self {
            account_id,
            cookie: Arc::new(cookie),
        })
    }

    pub fn account_id(&self) -> u64 {
        self.account_id
    }

    pub(super) fn cookie_header(&self) -> Result<HeaderValue, BilibiliError> {
        let mut value =
            HeaderValue::from_str(&self.cookie).map_err(|_| BilibiliError::InvalidResponse)?;
        value.set_sensitive(true);
        Ok(value)
    }

    /// The only serialization boundary is the system credential store.
    pub fn encode_secret(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.cookie.as_bytes().to_vec())
    }

    pub fn decode_secret(
        bytes: Zeroizing<Vec<u8>>,
        account_id: u64,
    ) -> Result<Self, BilibiliError> {
        let text = std::str::from_utf8(&bytes).map_err(|_| BilibiliError::InvalidResponse)?;
        Self::from_cookie(Zeroizing::new(text.to_string()), Some(account_id))
    }
}

fn valid_cookie_value(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(
            |byte| matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e),
        )
}

pub struct QrCode {
    key: Zeroizing<String>,
    pub png: Vec<u8>,
}
impl fmt::Debug for QrCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("QrCode([redacted])")
    }
}

#[derive(Debug)]
pub enum PollResult {
    WaitingScan,
    WaitingConfirmation,
    Expired,
    Confirmed(AuthSession),
}

#[derive(Deserialize)]
struct Envelope<T> {
    code: i64,
    data: Option<T>,
}
impl<T> Envelope<T> {
    fn into_data(self) -> Result<T, BilibiliError> {
        if self.code != 0 {
            return Err(BilibiliError::Api(self.code));
        }
        self.data.ok_or(BilibiliError::InvalidResponse)
    }
}

#[derive(Deserialize)]
struct Generated {
    url: ZeroizingText,
    qrcode_key: ZeroizingText,
}

// serde can construct this wrapper, but cannot serialize it or print it.
struct ZeroizingText(Zeroizing<String>);
impl<'de> Deserialize<'de> for ZeroizingText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

#[derive(Deserialize)]
struct PollData {
    code: i64,
}
#[derive(Deserialize)]
struct NavData {
    #[serde(rename = "isLogin")]
    is_login: bool,
    #[serde(default)]
    mid: u64,
    #[serde(default)]
    uname: String,
}

impl BilibiliClient {
    pub async fn generate_qr(&self) -> Result<QrCode, BilibiliError> {
        let response = self
            .get(true, "/x/passport-login/web/qrcode/generate", &[], None)
            .await?;
        let generated = Self::json::<Envelope<Generated>>(response)
            .await?
            .into_data()?;
        let url = url::Url::parse(&generated.url.0).map_err(|_| BilibiliError::InvalidResponse)?;
        if url.scheme() != "https"
            || url.host_str() != Some("account.bilibili.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || generated.qrcode_key.0.is_empty()
            || generated.qrcode_key.0.len() > 256
            || !generated
                .qrcode_key
                .0
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(BilibiliError::InvalidResponse);
        }
        let png = qr_png(&generated.url.0)?;
        Ok(QrCode {
            key: generated.qrcode_key.0,
            png,
        })
    }

    pub async fn poll_qr(&self, qr: &QrCode) -> Result<PollResult, BilibiliError> {
        let response = self
            .get(
                true,
                "/x/passport-login/web/qrcode/poll",
                &[("qrcode_key", &qr.key)],
                None,
            )
            .await?;
        let headers = response.headers().clone();
        let code = Self::json::<Envelope<PollData>>(response)
            .await?
            .into_data()?
            .code;
        match code {
            86101 => Ok(PollResult::WaitingScan),
            86090 => Ok(PollResult::WaitingConfirmation),
            86038 => Ok(PollResult::Expired),
            0 => AuthSession::from_headers(&headers).map(PollResult::Confirmed),
            other => Err(BilibiliError::Api(other)),
        }
    }

    pub async fn verify_account(&self, session: &AuthSession) -> Result<Account, BilibiliError> {
        let response = self
            .get(false, "/x/web-interface/nav", &[], Some(session))
            .await?;
        let nav = Self::json::<Envelope<NavData>>(response)
            .await?
            .into_data()?;
        if !nav.is_login || nav.mid == 0 {
            return Err(BilibiliError::NotAuthenticated);
        }
        if nav.mid != session.account_id {
            return Err(BilibiliError::AccountMismatch);
        }
        Ok(Account {
            id: nav.mid,
            name: nav.uname,
        })
    }
}

fn qr_png(text: &str) -> Result<Vec<u8>, BilibiliError> {
    let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium)
        .map_err(|_| BilibiliError::InvalidResponse)?;
    let border = 4;
    let scale = 6;
    let side = ((qr.size() + border * 2) * scale) as u32;
    let image = image::GrayImage::from_fn(side, side, |x, y| {
        image::Luma([
            if qr.get_module(x as i32 / scale - border, y as i32 / scale - border) {
                0
            } else {
                255
            },
        ])
    });
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| BilibiliError::InvalidResponse)?;
    Ok(png.into_inner())
}

#[cfg(test)]
mod tests;
