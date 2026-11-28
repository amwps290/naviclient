//! WebDAV HTTP client (`PROPFIND`, `Range` tag peeking, cover/lyrics GET) and XML parsing.

use std::io::Cursor;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, RANGE};
use reqwest::Method;
use url::Url;

use crate::models::network::NetworkRoute;

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:resourcetype/>
    <d:getcontentlength/>
    <d:getetag/>
    <d:getlastmodified/>
  </d:prop>
</d:propfind>"#;

const TAG_PEEK_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DavEntry {
    pub(super) url: Url,
    pub(super) decoded_path: String,
    pub(super) file_name: String,
    pub(super) is_collection: bool,
    pub(super) content_length: Option<u64>,
    pub(super) etag: Option<String>,
    pub(super) last_modified: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PeekedAudioTags {
    pub(super) title: Option<String>,
    pub(super) artist: Option<String>,
    pub(super) album_artist: Option<String>,
    pub(super) album: Option<String>,
    pub(super) genre: Option<String>,
    pub(super) track: Option<i32>,
    pub(super) disc: Option<i32>,
    pub(super) year: Option<i32>,
    pub(super) duration: Option<i32>,
    pub(super) bit_rate: Option<i32>,
}

#[derive(Clone)]
pub(super) struct WebDavClient {
    http: reqwest::Client,
    username: String,
    password: String,
}

impl WebDavClient {
    pub(super) fn new(http: reqwest::Client, username: String, password: String) -> Self {
        Self {
            http,
            username,
            password,
        }
    }

    pub(super) async fn propfind_depth_1(&self, dir_url: &Url) -> Result<Vec<DavEntry>> {
        let method = Method::from_bytes(b"PROPFIND").expect("valid HTTP method");
        let mut req_url = dir_url.clone();
        if !req_url.path().ends_with('/') {
            let with_slash = format!("{}/", req_url.path());
            req_url.set_path(&with_slash);
        }
        let mut headers = build_auth_headers(&self.username, &self.password);
        headers.insert("Depth", HeaderValue::from_static("1"));
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/xml; charset=utf-8"),
        );

        let response = self
            .http
            .request(method, req_url.clone())
            .headers(headers)
            .body(PROPFIND_BODY)
            .send()
            .await
            .map_err(|error| anyhow!("WebDAV PROPFIND failed: {}", error.without_url()))?;
        let status = response.status();
        if !status.is_success() && status.as_u16() != 207 {
            anyhow::bail!("WebDAV server returned HTTP {status}");
        }
        let body = response
            .text()
            .await
            .map_err(|error| anyhow!("Failed to read WebDAV XML response: {}", error.without_url()))?;
        parse_propfind_xml(&body, &req_url)
    }

    pub(super) async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .headers(build_auth_headers(&self.username, &self.password))
            .send()
            .await
            .map_err(|error| anyhow!("WebDAV GET failed: {}", error.without_url()))?;
        if !response.status().is_success() {
            anyhow::bail!("WebDAV GET returned HTTP {}", response.status());
        }
        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|error| anyhow!("Failed to read WebDAV response: {}", error.without_url()))
    }

    /// Best-effort partial HTTP `Range` request of the first 128 KiB to extract audio tags
    /// without downloading the full track. Returns `None` if tags cannot be parsed from header.
    pub(super) async fn peek_audio_tags(
        &self,
        url: &Url,
        file_name: &str,
    ) -> Option<PeekedAudioTags> {
        let mut headers = build_auth_headers(&self.username, &self.password);
        headers.insert(
            RANGE,
            HeaderValue::from_str(&format!("bytes=0-{}", TAG_PEEK_BYTES - 1)).ok()?,
        );
        let response = self
            .http
            .get(url.clone())
            .headers(headers)
            .timeout(Duration::from_secs(6))
            .send()
            .await
            .ok()?;
        let status = response.status();
        if status.as_u16() != 206 && !status.is_success() {
            return None;
        }
        let bytes = response.bytes().await.ok()?;
        if bytes.is_empty() || bytes.len() > TAG_PEEK_BYTES * 2 {
            return None;
        }
        let ext = file_name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        parse_tags_from_prefix(bytes.to_vec(), ext.as_deref())
    }
}

pub(super) fn parse_tags_from_prefix(
    bytes: Vec<u8>,
    ext_hint: Option<&str>,
) -> Option<PeekedAudioTags> {
    let cursor = Cursor::new(bytes);
    let mut probe = Probe::new(cursor).guess_file_type().ok()?;
    if probe.file_type().is_none() {
        if let Some(ext) = ext_hint {
            if let Some(ft) = lofty::file::FileType::from_ext(ext) {
                probe = probe.set_file_type(ft);
            }
        }
    }
    let tagged = probe.read().ok()?;
    let props = tagged.properties();
    let mut out = PeekedAudioTags {
        duration: i32::try_from(props.duration().as_secs())
            .ok()
            .filter(|secs| *secs > 0),
        bit_rate: props
            .audio_bitrate()
            .or_else(|| props.overall_bitrate())
            .and_then(|kbps| i32::try_from(kbps).ok())
            .filter(|kbps| *kbps > 0),
        ..Default::default()
    };
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        out.title = tag
            .title()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        out.artist = tag
            .artist()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        out.album_artist = tag
            .get_string(ItemKey::AlbumArtist)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        out.album = tag
            .album()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        out.genre = tag
            .genre()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        out.track = tag
            .track()
            .and_then(|n| i32::try_from(n).ok())
            .filter(|n| *n > 0);
        out.disc = tag
            .disk()
            .and_then(|n| i32::try_from(n).ok())
            .filter(|n| *n > 0);
        out.year = tag.date().map(|d| d.year as i32).filter(|y| *y > 0);
    }
    Some(out)
}

pub(super) fn build_http_client(route: &NetworkRoute) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30));
    match route {
        NetworkRoute::System => {}
        NetworkRoute::Direct => {
            builder = builder.no_proxy();
        }
        NetworkRoute::Proxy(proxy_url) => {
            builder = builder.proxy(
                reqwest::Proxy::all(proxy_url)
                    .map_err(|e| anyhow!("invalid WebDAV proxy: {e}"))?,
            );
        }
    }
    builder.build().context("failed to build WebDAV HTTP client")
}

pub(super) fn build_auth_headers(username: &str, password: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if !username.trim().is_empty() || !password.is_empty() {
        let token = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{password}", username.trim()));
        if let Ok(value) = HeaderValue::from_str(&format!("Basic {token}")) {
            headers.insert(AUTHORIZATION, value);
        }
    }
    headers
}

pub(super) fn join_webdav_url(base_url: &str, root_path: &str) -> Result<Url> {
    let mut url = Url::parse(base_url).map_err(|e| anyhow!("Invalid WebDAV base URL: {e}"))?;
    let trimmed_root = root_path.trim().trim_matches('/');
    if !trimmed_root.is_empty() {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow!("WebDAV URL cannot be a base"))?;
        segments.pop_if_empty();
        for seg in trimmed_root.split('/').filter(|s| !s.is_empty()) {
            segments.push(seg);
        }
    }
    Ok(url)
}

pub(super) fn parse_propfind_xml(xml: &str, request_url: &Url) -> Result<Vec<DavEntry>> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|error| anyhow!("Invalid WebDAV PROPFIND XML: {error}"))?;
    let request_path = normalize_compare_path(request_url.path());
    let mut entries = Vec::new();

    for node in doc.descendants() {
        if !node.is_element() || !node.tag_name().name().eq_ignore_ascii_case("response") {
            continue;
        }
        let Some(href_raw) = child_text_ignore_case(node, "href") else {
            continue;
        };
        let href = href_raw.trim();
        if href.is_empty() {
            continue;
        }
        let Ok(resolved_url) = request_url.join(href) else {
            continue;
        };
        let resolved_path = normalize_compare_path(resolved_url.path());
        // Skip the requested directory itself.
        if resolved_path == request_path {
            continue;
        }

        let mut is_collection = resolved_url.path().ends_with('/');
        let mut content_length = None;
        let mut etag = None;
        let mut last_modified = None;

        for prop_node in node.descendants().filter(|n| n.is_element()) {
            let name = prop_node.tag_name().name();
            if name.eq_ignore_ascii_case("collection") {
                is_collection = true;
            } else if name.eq_ignore_ascii_case("getcontentlength") {
                if let Some(text) = prop_node.text().map(str::trim) {
                    content_length = text.parse::<u64>().ok();
                }
            } else if name.eq_ignore_ascii_case("getetag") {
                etag = prop_node
                    .text()
                    .map(|t| t.trim().trim_matches('"').to_string())
                    .filter(|t| !t.is_empty());
            } else if name.eq_ignore_ascii_case("getlastmodified") {
                last_modified = prop_node
                    .text()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
            }
        }

        let decoded_path = percent_decode(resolved_url.path());
        let file_name = decoded_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        if file_name.is_empty() {
            continue;
        }

        entries.push(DavEntry {
            url: resolved_url,
            decoded_path,
            file_name,
            is_collection,
            content_length,
            etag,
            last_modified,
        });
    }

    Ok(entries)
}

fn child_text_ignore_case<'a>(node: roxmltree::Node<'a, 'a>, local_name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name().eq_ignore_ascii_case(local_name))
        .and_then(|child| child.text())
}

fn normalize_compare_path(path: &str) -> String {
    percent_decode(path).trim_end_matches('/').to_string()
}

pub(super) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
