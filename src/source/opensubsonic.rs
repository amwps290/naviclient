//! Adapter preserving the tested Subsonic protocol implementation.

use anyhow::{ensure, Result};

use super::{AudioQuality, HttpResource, MediaKey, PlaybackResource, ProviderKind, SourceId};
use crate::api::OpenSubsonicProvider;
use crate::models::{Song, TranscodingQuality};
use crate::playback_timeline::song_duration_hint;

#[derive(Clone)]
pub struct OpenSubsonicAdapter {
    id: SourceId,
    provider: OpenSubsonicProvider,
}

impl OpenSubsonicAdapter {
    pub fn new(profile_id: &str, url: &str, username: &str, password: &str) -> Result<Self> {
        ensure!(!profile_id.trim().is_empty(), "source profile ID is empty");
        Ok(Self {
            id: SourceId::new(ProviderKind::OpenSubsonic, profile_id),
            provider: OpenSubsonicProvider::new(url, username, password)?,
        })
    }

    pub fn id(&self) -> &SourceId {
        &self.id
    }

    pub(super) fn provider(&self) -> &OpenSubsonicProvider {
        &self.provider
    }

    pub(super) fn resolve_playback(
        &self,
        song: &Song,
        configured: TranscodingQuality,
    ) -> Result<PlaybackResource> {
        ensure!(!song.id.trim().is_empty(), "media ID is empty");
        ensure!(song.belongs_to(&self.id), "media belongs to another source");
        let effective = TranscodingQuality::effective_for(
            configured,
            song.suffix.as_deref(),
            song.content_type.as_deref(),
        );
        let estimated = effective.max_bit_rate().is_some();
        let http = HttpResource::new(
            self.provider
                .stream_url(&song.id, effective.max_bit_rate())?,
        )
        .with_estimated_length(estimated)
        .with_mime_hint(if estimated {
            Some("audio/mpeg".into())
        } else {
            song.content_type.clone()
        });
        Ok(PlaybackResource {
            media: MediaKey::new(self.id.clone(), &song.id),
            representation: effective.cache_profile().to_string(),
            http,
            backups: Vec::new(),
            quality: AudioQuality {
                codec: if effective.max_bit_rate().is_some() {
                    Some("mp3".into())
                } else {
                    song.suffix.clone()
                },
                bandwidth: effective
                    .max_bit_rate()
                    .map(|kbps| u64::from(kbps) * 1_000)
                    .or_else(|| {
                        song.bit_rate
                            .and_then(|kbps| u64::try_from(kbps).ok())
                            .map(|kbps| kbps * 1_000)
                    }),
                ..AudioQuality::default()
            },
            duration: song_duration_hint(song),
            expires_at: None,
        })
    }
}
