//! Loading policy and cache orchestration, without audio reconnection side effects.

use super::{
    cache::LyricsCache,
    lrclib::Lrclib,
    matching,
    provider::{LyricsProvider, ProviderError, Query},
    Document, LyricsPolicy, LyricsSettings,
};
use crate::{
    models::{CapabilitySupport, Song},
    source::{MediaKey, MusicSource},
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LyricsStatus {
    #[default]
    Empty,
    ReadingSource,
    SearchingOnline,
    NoMatch,
    MetadataNeeded,
    TitleNeeded,
    Ambiguous,
    SourceUnavailable,
    SourceTimedOut,
    Unavailable,
    RateLimited(u64),
    Instrumental,
    Ready,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadOutcome {
    pub document: Option<Document>,
    pub status: LyricsStatus,
    pub warning: Option<LyricsStatus>,
    pub error: Option<String>,
}

impl LoadOutcome {
    pub fn ready(document: Document) -> Self {
        let status = if document.instrumental {
            LyricsStatus::Instrumental
        } else {
            LyricsStatus::Ready
        };
        Self {
            document: Some(document),
            status,
            warning: None,
            error: None,
        }
    }
    pub fn empty(status: LyricsStatus) -> Self {
        Self {
            document: None,
            status,
            warning: None,
            error: None,
        }
    }
}

enum NativeError {
    TimedOut,
    Request(String),
}

impl NativeError {
    fn outcome(self) -> LoadOutcome {
        let (status, error) = match self {
            Self::TimedOut => (
                LyricsStatus::SourceTimedOut,
                "lyrics request timed out after 5 seconds".into(),
            ),
            Self::Request(error) => (LyricsStatus::SourceUnavailable, error),
        };
        LoadOutcome {
            error: Some(error),
            ..LoadOutcome::empty(status)
        }
    }
}

async fn native_lyrics(
    request: impl std::future::Future<Output = anyhow::Result<crate::models::Lyrics>>,
) -> Result<Option<Document>, NativeError> {
    let lyrics = tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .map_err(|_| NativeError::TimedOut)?
        .map_err(|error| NativeError::Request(crate::msg::error_message(error)))?;
    Ok((!lyrics.lines.is_empty()).then(|| Document::native(lyrics)))
}

impl From<ProviderError> for LyricsStatus {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::RateLimited { retry_seconds } => Self::RateLimited(retry_seconds),
            _ => Self::Unavailable,
        }
    }
}

pub(crate) struct LyricsService {
    pub settings: LyricsSettings,
    pub provider: Result<Lrclib, ProviderError>,
    pub cache: LyricsCache,
}

impl LyricsService {
    pub async fn load(
        &self,
        song: &Song,
        media: &MediaKey,
        source: Option<&MusicSource>,
        structured: CapabilitySupport,
        refresh: bool,
        progress: impl Fn(LyricsStatus, Option<Document>),
    ) -> LoadOutcome {
        if let Some(document) = self.cache.selected(media) {
            return LoadOutcome::ready(document);
        }
        let query = Query::for_song(song);
        let preference = self.cache.preference(media);
        let mut online_deadline = None;
        if self.settings.enabled && self.settings.policy != LyricsPolicy::Manual {
            if let (Some(id), Ok(provider)) = (preference.record_id, &self.provider) {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                online_deadline = Some(deadline);
                if let Ok(Ok(Some(record))) =
                    tokio::time::timeout_at(deadline, provider.fetch(id)).await
                {
                    if record.id == id && record.usable() {
                        let mut document = Document::online(record);
                        document.manual = true;
                        document.plain = preference.plain;
                        document.offset_ms = preference.offset_ms;
                        document.query = preference.query.clone();
                        if let Err(error) = self.cache.remember(media, &document) {
                            log::warn!("could not recover remembered lyrics cache: {error:#}");
                        }
                        return LoadOutcome::ready(document);
                    }
                }
            }
        }
        let native = || async {
            match source.filter(|s| s.capabilities().lyrics) {
                Some(source) => native_lyrics(source.lyrics(song, structured)).await,
                None => Ok(None),
            }
        };
        let mut native_error = None;
        let online_enabled = self.settings.enabled && self.settings.policy != LyricsPolicy::Manual;
        if !online_enabled || self.settings.policy == LyricsPolicy::SourceFirst {
            progress(LyricsStatus::ReadingSource, None);
            match native().await {
                Ok(Some(mut document)) => {
                    document.plain = preference.plain;
                    document.offset_ms = preference.offset_ms;
                    return LoadOutcome::ready(document);
                }
                Err(error) => native_error = Some(error),
                Ok(None) => {}
            }
        }
        let mut outcome = if !online_enabled {
            LoadOutcome::empty(LyricsStatus::Empty)
        } else if !query.automatic() {
            LoadOutcome::empty(LyricsStatus::MetadataNeeded)
        } else {
            progress(LyricsStatus::SearchingOnline, None);
            let deadline = online_deadline
                .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(8));
            match tokio::time::timeout_at(deadline, self.online(&query, refresh)).await {
                Ok(outcome) => outcome,
                Err(_) => LoadOutcome::empty(LyricsStatus::Unavailable),
            }
        };
        if outcome.document.is_none()
            && online_enabled
            && self.settings.policy == LyricsPolicy::OnlineFirst
        {
            progress(LyricsStatus::ReadingSource, None);
            match native().await {
                Ok(Some(document)) => {
                    let warning = Some(outcome.status);
                    outcome = LoadOutcome::ready(document);
                    outcome.warning = warning;
                }
                Err(error) => native_error = Some(error),
                Ok(None) => {}
            }
        }
        if outcome.document.is_none() {
            if let Some(error) = native_error {
                let online_status = online_enabled.then_some(outcome.status);
                outcome = error.outcome();
                outcome.warning = online_status;
            }
        }
        if let Some(document) = outcome.document.as_mut() {
            document.plain = preference.plain;
            document.offset_ms = preference.offset_ms;
        }
        outcome
    }

    async fn online(&self, query: &Query, refresh: bool) -> LoadOutcome {
        if !refresh {
            if let Some(hit) = self.cache.automatic(query) {
                return hit
                    .map(Document::online)
                    .map(LoadOutcome::ready)
                    .unwrap_or_else(|| LoadOutcome::empty(LyricsStatus::NoMatch));
            }
        }
        let provider = match &self.provider {
            Ok(p) => p,
            Err(e) => return LoadOutcome::empty((*e).into()),
        };
        match provider.lookup(query).await {
            Ok(Some(record)) if matching::assess(query, &record).trusted => {
                if let Err(error) = self.cache.save_automatic(query, Some(record.clone())) {
                    log::warn!("lyrics automatic cache save failed: {error:#}");
                }
                return LoadOutcome::ready(Document::online(record));
            }
            Err(error) => return LoadOutcome::empty(error.into()),
            _ => {}
        }
        let records = match provider.search(query).await {
            Ok(records) => records,
            Err(error) => return LoadOutcome::empty(error.into()),
        };
        match matching::automatic(query, records) {
            Err(()) => LoadOutcome::empty(LyricsStatus::Ambiguous),
            Ok(record) => {
                if let Err(error) = self.cache.save_automatic(query, record.clone()) {
                    log::warn!("lyrics automatic cache save failed: {error:#}");
                }
                record
                    .map(Document::online)
                    .map(LoadOutcome::ready)
                    .unwrap_or_else(|| LoadOutcome::empty(LyricsStatus::NoMatch))
            }
        }
    }
}

#[cfg(test)]
mod tests;
