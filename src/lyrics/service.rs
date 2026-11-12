//! Loading policy and cache orchestration, without audio reconnection side effects.

use super::{
    cache::LyricsCache,
    matching,
    online::Provider,
    provider::{LyricsProvider, ProviderError, ProviderId, Query, SearchOutcome},
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
    SourcesNeeded,
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
    pub provider: Result<Vec<Provider>, ProviderError>,
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
            if let (Some(id), Ok(providers)) = (preference.record_id, &self.provider) {
                let provider = providers.iter().find(|provider| {
                    provider.id() == preference.provider
                        && self.settings.sources.contains(&provider.id())
                });
                let deadline = tokio::time::Instant::now() + self.online_budget();
                online_deadline = Some(deadline);
                if let Some(provider) = provider {
                    if let Ok(Ok(Some(record))) = tokio::time::timeout(
                        Duration::from_secs(8),
                        provider.fetch(id, preference.query.as_ref().or(Some(&query))),
                    )
                    .await
                    {
                        if record.id == id
                            && record.source == preference.provider
                            && record.usable()
                        {
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
        } else if self.settings.enabled_sources().is_empty() {
            LoadOutcome::empty(LyricsStatus::SourcesNeeded)
        } else if !query.automatic() {
            LoadOutcome::empty(LyricsStatus::MetadataNeeded)
        } else {
            progress(LyricsStatus::SearchingOnline, None);
            let deadline = online_deadline
                .unwrap_or_else(|| tokio::time::Instant::now() + self.online_budget());
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

    fn online_budget(&self) -> Duration {
        Duration::from_secs(8 * self.settings.enabled_sources().len().max(1) as u64)
    }

    async fn online(&self, query: &Query, refresh: bool) -> LoadOutcome {
        let sources = self.settings.enabled_sources();
        if sources.is_empty() {
            return LoadOutcome::empty(LyricsStatus::SourcesNeeded);
        }
        let mut failure = None;
        for id in sources {
            let outcome =
                tokio::time::timeout(Duration::from_secs(8), self.online_from(id, query, refresh))
                    .await
                    .unwrap_or_else(|_| LoadOutcome::empty(LyricsStatus::Unavailable));
            if outcome.document.is_some() || outcome.status == LyricsStatus::Ambiguous {
                return outcome;
            }
            if outcome.status != LyricsStatus::NoMatch {
                failure = Some(outcome.status);
            }
        }
        LoadOutcome::empty(failure.unwrap_or(LyricsStatus::NoMatch))
    }

    async fn online_from(&self, id: ProviderId, query: &Query, refresh: bool) -> LoadOutcome {
        if !refresh {
            if let Some(hit) = self.cache.automatic_from(id, query) {
                return hit
                    .map(Document::online)
                    .map(LoadOutcome::ready)
                    .unwrap_or_else(|| LoadOutcome::empty(LyricsStatus::NoMatch));
            }
        }
        let providers = match &self.provider {
            Ok(p) => p,
            Err(e) => return LoadOutcome::empty((*e).into()),
        };
        let Some(provider) = providers.iter().find(|provider| provider.id() == id) else {
            return LoadOutcome::empty(LyricsStatus::Unavailable);
        };
        match provider.lookup(query).await {
            Ok(Some(record)) if matching::assess(query, &record).trusted => {
                if let Err(error) = self
                    .cache
                    .save_automatic_from(id, query, Some(record.clone()))
                {
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
                if let Err(error) = self.cache.save_automatic_from(id, query, record.clone()) {
                    log::warn!("lyrics automatic cache save failed: {error:#}");
                }
                record
                    .map(Document::online)
                    .map(LoadOutcome::ready)
                    .unwrap_or_else(|| LoadOutcome::empty(LyricsStatus::NoMatch))
            }
        }
    }

    pub async fn search(
        &self,
        query: &Query,
        source: Option<ProviderId>,
    ) -> Result<SearchOutcome, ProviderError> {
        if !self.settings.enabled {
            return Err(ProviderError::Unavailable);
        }
        let providers = self.provider.as_ref().map_err(|error| *error)?;
        let mut tasks = tokio::task::JoinSet::new();
        for (index, id) in self
            .settings
            .enabled_sources()
            .into_iter()
            .enumerate()
            .filter(|(_, id)| source.is_none_or(|source| source == *id))
        {
            let query = query.clone();
            let provider = providers
                .iter()
                .find(|provider| provider.id() == id)
                .cloned();
            tasks.spawn(async move {
                let result = match provider {
                    Some(provider) => {
                        tokio::time::timeout(Duration::from_secs(8), provider.search(&query))
                            .await
                            .unwrap_or(Err(ProviderError::Unavailable))
                    }
                    None => Err(ProviderError::Unavailable),
                };
                (index, id, result)
            });
        }
        if tasks.is_empty() {
            return Err(ProviderError::Unavailable);
        }
        let mut results = Vec::new();
        while let Some(result) = tasks.join_next().await {
            results.push(result.map_err(|_| ProviderError::Unavailable)?);
        }
        results.sort_by_key(|(index, _, _)| *index);
        let mut outcome = SearchOutcome::default();
        for (_, id, result) in results {
            match result {
                Ok(mut records) => {
                    records.truncate(6);
                    outcome.records.extend(records);
                }
                Err(error) => outcome.failures.push((id, error)),
            }
        }
        outcome.records = matching::ranked(query, outcome.records);
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests;
