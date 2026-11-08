//! Parse only DASH audio; `video`, progressive `durl`, FLAC and Dolby cannot
//! silently become fallback media for the verified AAC-LC decoder path.

use super::{
    client::{BilibiliClient, BilibiliError, Envelope, REFERER, USER_AGENT},
    library::parse_media_id,
    BilibiliProvider,
};
use crate::{
    models::Song,
    source::{AudioQuality, CachePolicy, HttpResource, MediaKey, PlaybackResource},
};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Deserialize)]
struct PlayData {
    #[serde(default)]
    dash: Option<Dash>,
}
#[derive(Clone, Deserialize)]
struct Dash {
    #[serde(default)]
    audio: Option<Vec<Audio>>,
}
#[derive(Clone)]
struct Audio {
    id: u64,
    codecs: String,
    bandwidth: u64,
    mime: String,
    url: String,
    backups: Vec<String>,
}

// Real responses may contain BOTH naming styles, and null backup arrays.
// serde aliases alone reject those valid responses as duplicate fields.
impl<'de> Deserialize<'de> for Audio {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            id: u64,
            codecs: String,
            bandwidth: u64,
            #[serde(rename = "mimeType")]
            camel_mime: Option<String>,
            mime_type: Option<String>,
            #[serde(rename = "baseUrl")]
            camel_url: Option<String>,
            base_url: Option<String>,
            #[serde(rename = "backupUrl")]
            camel_backups: Option<Vec<String>>,
            backup_url: Option<Vec<String>>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            id: wire.id,
            codecs: wire.codecs,
            bandwidth: wire.bandwidth,
            mime: wire
                .camel_mime
                .or(wire.mime_type)
                .ok_or_else(|| serde::de::Error::custom("missing audio MIME"))?,
            url: wire
                .camel_url
                .or(wire.base_url)
                .ok_or_else(|| serde::de::Error::custom("missing audio address"))?,
            backups: wire.camel_backups.or(wire.backup_url).unwrap_or_default(),
        })
    }
}

pub(super) async fn resolve(
    provider: &BilibiliProvider,
    song: &Song,
) -> Result<PlaybackResource, BilibiliError> {
    if song.source.as_ref() != Some(provider.id()) {
        return Err(BilibiliError::AccountMismatch);
    }
    let (bvid, cid) = parse_media_id(&song.id)?;
    let video = provider.video(bvid).await?;
    let part = video
        .pages
        .iter()
        .find(|part| part.cid == cid)
        .ok_or(BilibiliError::Unavailable)?;
    if song
        .bilibili_part
        .as_ref()
        .is_some_and(|part| part.bvid != bvid || part.cid != cid)
    {
        return Err(BilibiliError::InvalidResponse);
    }
    let cid_text = cid.to_string();
    let response = provider
        .client
        .get(
            false,
            "/x/player/playurl",
            &[
                ("bvid", bvid),
                ("cid", &cid_text),
                ("fnval", "4048"),
                ("fnver", "0"),
                ("fourk", "1"),
            ],
            Some(&provider.session),
        )
        .await?;
    let mut data = BilibiliClient::json::<Envelope<PlayData>>(response)
        .await?
        .into_data()?;
    let mut last_error = BilibiliError::UnsupportedAudio;
    // Supported representations are tried by declared bandwidth. CDN failures
    // can fall back to another representation before any decoder/cache starts.
    for _ in 0..8 {
        let mut resource = match select(
            provider,
            song,
            part.duration,
            data.clone(),
            SystemTime::now(),
        ) {
            Ok(resource) => resource,
            Err(_) => return Err(last_error),
        };
        let addresses: Vec<_> = std::iter::once(resource.http.clone())
            .chain(resource.backups.iter().cloned())
            .take(3)
            .collect();
        for (index, address) in addresses.iter().enumerate() {
            match provider.client.audio_access(address).await {
                Ok(()) => {
                    resource.http = address.clone();
                    resource.backups = addresses.into_iter().skip(index + 1).collect();
                    return Ok(resource);
                }
                Err(BilibiliError::RateLimited) => return Err(BilibiliError::RateLimited),
                Err(error) => last_error = error,
            }
        }
        if let Some(audio) = data.dash.as_mut().and_then(|dash| dash.audio.as_mut()) {
            audio.retain(|audio| {
                format!("dash-audio-{}-mp4a.40.2-{}", audio.id, audio.bandwidth)
                    != resource.representation
            });
        }
    }
    Err(last_error)
}

fn select(
    provider: &BilibiliProvider,
    song: &Song,
    duration: u64,
    data: PlayData,
    now: SystemTime,
) -> Result<PlaybackResource, BilibiliError> {
    let mut audio = data.dash.and_then(|dash| dash.audio).unwrap_or_default();
    audio.retain(|audio| {
        audio.codecs == "mp4a.40.2"
            && audio.mime == "audio/mp4"
            && audio.bandwidth > 0
            && audio.bandwidth <= 10_000_000
    });
    audio.sort_by(|left, right| {
        right
            .bandwidth
            .cmp(&left.bandwidth)
            .then_with(|| right.id.cmp(&left.id))
    });
    for audio in audio {
        let mut urls = Vec::new();
        for value in std::iter::once(&audio.url)
            .chain(audio.backups.iter())
            .take(9)
        {
            if let Some((url, expiry)) = safe_audio_url(value) {
                if expiry.is_some_and(|expiry| expiry <= now + Duration::from_secs(30)) {
                    continue;
                }
                if !urls.iter().any(|(existing, _)| existing == &url) {
                    urls.push((url, expiry));
                }
            }
        }
        if urls.is_empty() {
            continue;
        }
        let expires_at = urls.iter().filter_map(|(_, expiry)| *expiry).min();
        let mut resources = urls.into_iter().map(|(url, _)| resource(provider, url));
        let http = resources.next().ok_or(BilibiliError::UnsupportedAudio)?;
        return Ok(PlaybackResource {
            media: MediaKey::new(provider.id.clone(), &song.id),
            representation: format!("dash-audio-{}-mp4a.40.2-{}", audio.id, audio.bandwidth),
            http,
            backups: resources.collect(),
            quality: AudioQuality {
                codec: Some(audio.codecs),
                bandwidth: Some(audio.bandwidth),
                ..Default::default()
            },
            duration: Some(Duration::from_secs(duration)),
            expires_at,
        });
    }
    Err(BilibiliError::UnsupportedAudio)
}

fn resource(provider: &BilibiliProvider, url: String) -> HttpResource {
    let mut headers = HeaderMap::new();
    headers.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    headers.insert(header::REFERER, HeaderValue::from_static(REFERER));
    HttpResource::new(url)
        .with_headers(headers)
        .with_proxy(provider.proxy.clone())
        .with_mime_hint(Some("audio/mp4".into()))
        .with_cache_policy(CachePolicy::Temporary {
            max_bytes: 32 * 1024 * 1024,
        })
}

fn safe_audio_url(value: &str) -> Option<(String, Option<SystemTime>)> {
    let mut url = url::Url::parse(value).ok()?;
    let host = url.host_str()?;
    // Bilibili media CDNs only; no arbitrary URLs from API payloads or cookies
    // on CDN requests. Upgrade historical HTTP addresses to HTTPS.
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || !(host.ends_with(".bilivideo.com")
            || host.ends_with(".bilivideo.cn")
            || host.ends_with(".akamaized.net"))
    {
        return None;
    }
    let expiry = url
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("deadline"))
        .map(|(_, value)| value.parse::<u64>())
        .transpose()
        .ok()?
        .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)));
    url.set_scheme("https").ok()?;
    Some((url.to_string(), expiry))
}

#[cfg(test)]
mod tests;
