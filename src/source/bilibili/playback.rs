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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::source::resource::{
    timing::{EXPIRY_MARGIN, RESOLUTION_BUDGET},
    ResourceExpiry,
};

const MAX_AUDIO_PROBES: usize = 9;
const MAX_PROBES_PER_FORMAT: usize = 3;

/// The API's Date header and its receipt times give a calibrated lifetime for
/// signed addresses. Missing/invalid Date leaves expiry unknown; the CDN probe
/// then judges accessibility, instead of trusting a possibly wrong local clock.
#[derive(Clone, Copy)]
struct ServerClock {
    time: SystemTime,
    received: Instant,
    wall_received: SystemTime,
}

impl ServerClock {
    fn from_headers(
        headers: &HeaderMap,
        received: Instant,
        wall_received: SystemTime,
    ) -> Option<Self> {
        let date = headers.get(header::DATE)?.to_str().ok()?;
        let time = chrono::DateTime::parse_from_rfc2822(date).ok()?.into();
        Some(Self {
            time,
            received,
            wall_received,
        })
    }

    fn expiry(self, deadline: SystemTime) -> Option<ResourceExpiry> {
        ResourceExpiry::from_lifetime(
            self.received,
            self.wall_received,
            deadline.duration_since(self.time).unwrap_or_default(),
        )
    }
}

#[derive(Clone, Deserialize)]
struct PlayData {
    #[serde(default)]
    dash: Option<Dash>,
    timelength: Option<u64>,
}
#[derive(Clone, Deserialize)]
struct Dash {
    #[serde(default)]
    audio: Option<Vec<Audio>>,
    duration: Option<f64>,
}

impl PlayData {
    fn duration(&self, fallback: Option<Duration>) -> Option<Duration> {
        self.timelength
            .filter(|milliseconds| *milliseconds > 0 && *milliseconds <= i32::MAX as u64 * 1000)
            .map(Duration::from_millis)
            .or_else(|| {
                self.dash
                    .as_ref()
                    .and_then(|dash| dash.duration)
                    .filter(|seconds| {
                        seconds.is_finite() && *seconds > 0.0 && *seconds <= i32::MAX as f64
                    })
                    .map(Duration::from_secs_f64)
            })
            .or(fallback)
    }
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
    resolve_before(
        provider,
        song,
        tokio::time::Instant::now() + RESOLUTION_BUDGET,
    )
    .await
}

async fn resolve_before(
    provider: &BilibiliProvider,
    song: &Song,
    deadline: tokio::time::Instant,
) -> Result<PlaybackResource, BilibiliError> {
    if song.source.as_ref() != Some(provider.id()) {
        return Err(BilibiliError::AccountMismatch);
    }
    let (bvid, cid) = parse_media_id(&song.id)?;
    if song
        .bilibili_part
        .as_ref()
        .is_some_and(|part| part.bvid != bvid || part.cid != cid)
    {
        return Err(BilibiliError::InvalidResponse);
    }
    // Favorite loading already validated the video/part identity. Legacy or
    // incomplete entries still use /view before opening any audio address.
    let fallback_duration = if song.bilibili_part.is_some() {
        song.duration
            .filter(|duration| *duration > 0)
            .map(|duration| Duration::from_secs(duration as u64))
    } else {
        let video = tokio::time::timeout_at(deadline, provider.video(bvid))
            .await
            .map_err(|_| BilibiliError::Timeout)??;
        let part = video
            .pages
            .iter()
            .find(|part| part.cid == cid)
            .ok_or(BilibiliError::Unavailable)?;
        Some(Duration::from_secs(part.duration))
    };
    let cid_text = cid.to_string();
    let (data, clock) = tokio::time::timeout_at(deadline, async {
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
        let clock =
            ServerClock::from_headers(response.headers(), Instant::now(), SystemTime::now());
        let data = BilibiliClient::json::<Envelope<PlayData>>(response)
            .await?
            .into_data()?;
        Ok::<_, BilibiliError>((data, clock))
    })
    .await
    .map_err(|_| BilibiliError::Timeout)??;
    let duration = data.duration(fallback_duration);
    let candidates = select_candidates(provider, song, duration, data, clock, Instant::now())?;
    let mut last_error = BilibiliError::UnsupportedAudio;
    // Supported representations are tried by declared bandwidth. CDN failures
    // can fall back to a lower format. Keep a per-format limit so one format's
    // backups cannot consume the whole budget. Unprobed backups remain usable
    // by the reader after the successful address.
    let probes: Vec<_> = candidates
        .iter()
        .flat_map(|resource| {
            std::iter::once(&resource.http)
                .chain(&resource.backups)
                .take(MAX_PROBES_PER_FORMAT)
                .enumerate()
                .map(move |(index, address)| (resource, index, address))
        })
        .take(MAX_AUDIO_PROBES)
        .collect();
    for (candidate, index, address) in probes {
        if address.expired_at(Instant::now(), SystemTime::now()) {
            last_error = BilibiliError::ResourceExpired;
            continue;
        }
        let result = tokio::time::timeout_at(deadline, provider.client.audio_access(address)).await;
        let result = match result {
            Ok(result) => result,
            // Preserve a concrete CDN error when the overall budget runs out.
            Err(_) => {
                return Err(if last_error == BilibiliError::UnsupportedAudio {
                    BilibiliError::Timeout
                } else {
                    last_error
                })
            }
        };
        match result {
            Ok(()) => {
                let mut resource = candidate.clone();
                resource.http = address.clone();
                resource.backups = candidate.backups.iter().skip(index).cloned().collect();
                return Ok(resource);
            }
            Err(BilibiliError::RateLimited) => return Err(BilibiliError::RateLimited),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn select_candidates(
    provider: &BilibiliProvider,
    song: &Song,
    duration: Option<Duration>,
    data: PlayData,
    clock: Option<ServerClock>,
    now: Instant,
) -> Result<Vec<PlaybackResource>, BilibiliError> {
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
    let mut candidates = Vec::new();
    let mut expired = false;
    let wall_now = SystemTime::now();
    for audio in audio {
        let mut urls = Vec::new();
        for value in std::iter::once(&audio.url)
            .chain(audio.backups.iter())
            .take(9)
        {
            if let Some((url, deadline)) = safe_audio_url(value) {
                let expiry = clock.and_then(|clock| deadline.and_then(|value| clock.expiry(value)));
                if expiry.is_some_and(|expiry| {
                    expiry.expired_at(now + EXPIRY_MARGIN, wall_now + EXPIRY_MARGIN)
                }) {
                    expired = true;
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
        let mut resources = urls
            .into_iter()
            .map(|(url, expiry)| resource(provider, url).with_expiry(expiry));
        let http = resources.next().ok_or(BilibiliError::UnsupportedAudio)?;
        candidates.push(PlaybackResource {
            media: MediaKey::new(provider.id.clone(), &song.id),
            representation: format!("dash-audio-{}-mp4a.40.2-{}", audio.id, audio.bandwidth),
            http,
            backups: resources.collect(),
            quality: AudioQuality {
                codec: Some(audio.codecs),
                bandwidth: Some(audio.bandwidth),
                ..Default::default()
            },
            duration,
        });
    }
    if candidates.is_empty() {
        Err(if expired {
            BilibiliError::ResourceExpired
        } else {
            BilibiliError::UnsupportedAudio
        })
    } else {
        Ok(candidates)
    }
}

fn resource(provider: &BilibiliProvider, url: String) -> HttpResource {
    let mut headers = HeaderMap::new();
    headers.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    headers.insert(header::REFERER, HeaderValue::from_static(REFERER));
    HttpResource::new(url)
        .with_headers(headers)
        .with_route(provider.route.clone())
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
            || host
                .strip_prefix("upos-")
                .and_then(|name| name.strip_suffix("-mirrorakam.akamaized.net"))
                .is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                }))
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
