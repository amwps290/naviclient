//! Fixed-origin, bounded account requests. Transport errors are redacted.

use super::auth::AuthSession;
use crate::models::network::NetworkRoute;
use reqwest::{header, Client, Response};
use rust_i18n::t;
use serde::de::DeserializeOwned;
use std::{fmt, time::Duration};

const RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
pub const USER_AGENT: &str = "Mozilla/5.0 Naviclient/0.1";
pub const REFERER: &str = "https://www.bilibili.com/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BilibiliError {
    Network,
    Timeout,
    Http(u16),
    Api(i64),
    InvalidResponse,
    InvalidProxy,
    NotAuthenticated,
    AccountMismatch,
    Expired,
    PermissionDenied,
    Unavailable,
    RegionRestricted,
    RateLimited,
    UnsupportedAudio,
    ResourceExpired,
    LimitExceeded,
}

impl fmt::Display for BilibiliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(code) => f.write_str(&t!("Bilibili HTTP error %{code}", code = code)),
            Self::Api(code) => f.write_str(&t!("Bilibili API error %{code}", code = code)),
            other => f.write_str(&match other {
                Self::Network => {
                    t!("Cannot reach Bilibili. Check the connection or proxy and retry.")
                }
                Self::Timeout => t!("Bilibili request timed out. Retry playback."),
                Self::InvalidResponse => {
                    t!("Bilibili returned an invalid response. Refresh and retry.")
                }
                Self::InvalidProxy => t!("Enter a valid proxy address."),
                Self::NotAuthenticated => t!("Bilibili login expired. Sign in again."),
                Self::AccountMismatch => {
                    t!("Scan with the same Bilibili account, or add a new account.")
                }
                Self::Expired => t!("QR code expired. Generate a new one."),
                Self::PermissionDenied => t!("Bilibili content is private or access is denied."),
                Self::Unavailable => t!("Bilibili video is deleted or unavailable."),
                Self::RegionRestricted => t!("Bilibili content is unavailable in this region."),
                Self::RateLimited => t!("Bilibili request rate limited. Retry later."),
                Self::UnsupportedAudio => t!("Bilibili has no supported independent AAC-LC audio."),
                Self::ResourceExpired => {
                    t!("A fresh playback address is required. Retry playback.")
                }
                Self::LimitExceeded => {
                    t!("Bilibili favorites exceed the loading limit. No partial queue was created.")
                }
                _ => unreachable!(),
            }),
        }
    }
}
impl std::error::Error for BilibiliError {}

impl BilibiliError {
    pub(super) fn content_api(code: i64) -> Self {
        match code {
            -101 => Self::NotAuthenticated,
            -403 | 62012 => Self::PermissionDenied,
            -404 | 62002 | 62004 => Self::Unavailable,
            -10403 => Self::RegionRestricted,
            -352 | -412 | -509 => Self::RateLimited,
            other => Self::Api(other),
        }
    }
}

fn transport_error(error: reqwest::Error) -> BilibiliError {
    if error.is_timeout() {
        BilibiliError::Timeout
    } else {
        BilibiliError::Network
    }
}

#[derive(serde::Deserialize)]
pub(super) struct Envelope<T> {
    code: i64,
    data: Option<T>,
}

impl<T> Envelope<T> {
    pub(super) fn into_data(self) -> Result<T, BilibiliError> {
        if self.code != 0 {
            return Err(BilibiliError::content_api(self.code));
        }
        self.data.ok_or(BilibiliError::InvalidResponse)
    }
}

#[derive(Clone)]
pub struct BilibiliClient {
    http: Client,
    audio_clients: crate::source::resource::clients::AsyncClients,
    #[cfg(test)]
    test_origin: Option<String>,
}

impl BilibiliClient {
    /// One audio byte establishes accessibility without opening video media or
    /// forwarding the account Cookie to a CDN. The body stays bounded.
    pub(super) async fn audio_access(
        &self,
        resource: &crate::source::HttpResource,
    ) -> Result<(), BilibiliError> {
        #[cfg(test)]
        let address = self.test_origin.as_ref().map(|origin| {
            let url = url::Url::parse(resource.url()).expect("validated audio URL");
            format!("{origin}{}", &url[url::Position::BeforePath..])
        });
        #[cfg(test)]
        let address = address.as_deref().unwrap_or(resource.url());
        #[cfg(not(test))]
        let address = resource.url();
        let client_resource = crate::source::HttpResource::new(address)
            .with_headers(resource.headers().clone())
            .with_route(resource.route().clone());
        let client = self
            .audio_clients
            .get(&client_resource)
            .map_err(|_| BilibiliError::Network)?;
        let mut response = client
            .get(address)
            .headers(resource.headers().clone())
            .header(header::RANGE, "bytes=0-0")
            .timeout(crate::source::resource::timing::AUDIO_PROBE_TIMEOUT)
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(if response.status().as_u16() == 429 {
                BilibiliError::RateLimited
            } else {
                BilibiliError::Http(response.status().as_u16())
            });
        }
        if response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|mime| mime.starts_with("text/") || mime.starts_with("application/json"))
        {
            return Err(BilibiliError::UnsupportedAudio);
        }
        if response
            .chunk()
            .await
            .map_err(transport_error)?
            .is_none_or(|chunk| chunk.is_empty())
        {
            return Err(BilibiliError::InvalidResponse);
        }
        Ok(())
    }
    pub(super) async fn artwork(&self, url: &str) -> Result<Vec<u8>, BilibiliError> {
        let url = super::library::safe_artwork(url).ok_or(BilibiliError::InvalidResponse)?;
        let mut response = self
            .http
            .get(url)
            .header(header::USER_AGENT, USER_AGENT)
            .header(header::REFERER, REFERER)
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(BilibiliError::Http(response.status().as_u16()));
        }
        let limit = 8 * 1024 * 1024;
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(BilibiliError::InvalidResponse);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(BilibiliError::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    pub fn new(route: &NetworkRoute) -> Result<Self, BilibiliError> {
        let builder = Client::builder()
            .referer(false)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20));
        let builder = crate::source::resource::clients::route_async(builder, route)
            .map_err(|_| BilibiliError::InvalidProxy)?;
        Ok(Self {
            http: builder.build().map_err(|_| BilibiliError::Network)?,
            audio_clients: Default::default(),
            #[cfg(test)]
            test_origin: None,
        })
    }

    pub(super) async fn get(
        &self,
        passport: bool,
        path: &str,
        params: &[(&str, &str)],
        session: Option<&AuthSession>,
    ) -> Result<Response, BilibiliError> {
        let origin = if passport {
            "https://passport.bilibili.com"
        } else {
            "https://api.bilibili.com"
        };
        #[cfg(test)]
        let origin = self.test_origin.as_deref().unwrap_or(origin);
        let mut request = self
            .http
            .get(format!("{origin}{path}"))
            .query(params)
            .header(header::USER_AGENT, USER_AGENT)
            .header(header::REFERER, REFERER);
        if let Some(session) = session {
            request = request.header(header::COOKIE, session.cookie_header()?);
        }
        let response = request.send().await.map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(if response.status().as_u16() == 429 {
                BilibiliError::RateLimited
            } else {
                BilibiliError::Http(response.status().as_u16())
            });
        }
        Ok(response)
    }

    pub(super) async fn json<T: DeserializeOwned>(
        mut response: Response,
    ) -> Result<T, BilibiliError> {
        if response
            .content_length()
            .is_some_and(|length| length > RESPONSE_LIMIT as u64)
        {
            return Err(BilibiliError::InvalidResponse);
        }
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if bytes.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
                return Err(BilibiliError::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| BilibiliError::InvalidResponse)
    }

    #[cfg(test)]
    pub(super) fn local(origin: String) -> Self {
        let mut client = Self::new(&NetworkRoute::Direct).unwrap();
        client.test_origin = Some(origin);
        client
    }
}
