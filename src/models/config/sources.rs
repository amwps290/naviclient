//! Idempotent source-profile migration while legacy server storage is retained.

use std::collections::HashSet;

use super::{Config, ProviderKind, SourceId, SourceProfile};
use serde::Deserialize;

/// A damaged new source entry must not discard legacy server credentials or
/// unrelated global settings when loading an otherwise valid configuration.
pub(super) fn deserialize_profiles<'de, D>(deserializer: D) -> Result<Vec<SourceProfile>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let Some(items) = value.as_array() else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|item| match serde_json::from_value(item.clone()) {
            Ok(profile) => Some(profile),
            Err(_) => {
                log::warn!("ignoring an unsupported or invalid source profile");
                None
            }
        })
        .collect())
}

pub(super) fn deserialize_active<'de, D>(deserializer: D) -> Result<Option<SourceId>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or(None))
}

impl Config {
    pub(super) fn sync_sources(&mut self) {
        let mut identities = HashSet::new();
        self.sources.retain(|source| {
            let id = source.id();
            let valid = match source {
                SourceProfile::OpenSubsonic { server_id } => {
                    self.servers.iter().any(|server| &server.id == server_id)
                }
                SourceProfile::Bilibili { id, .. } => !id.trim().is_empty(),
            };
            valid && identities.insert(id)
        });
        for server in &self.servers {
            let id = SourceId::new(ProviderKind::OpenSubsonic, &server.id);
            if identities.insert(id) {
                self.sources.push(SourceProfile::OpenSubsonic {
                    server_id: server.id.clone(),
                });
            }
        }
        if self
            .playback_source
            .as_ref()
            .is_some_and(|source| !identities.contains(source))
        {
            self.playback_source = None;
        }
        if !self
            .active_source
            .as_ref()
            .is_some_and(|source| identities.contains(source))
        {
            self.active_source = self
                .active_server
                .as_ref()
                .map(|id| SourceId::new(ProviderKind::OpenSubsonic, id))
                .filter(|id| identities.contains(id))
                .or_else(|| self.sources.first().map(SourceProfile::id));
        }
        if let Some(source) = self
            .active_source
            .as_ref()
            .filter(|source| source.provider == ProviderKind::OpenSubsonic)
        {
            self.active_server = Some(source.profile_id.clone());
        }
    }

    /// Select a browsing source. Bilibili never populates server credentials.
    pub fn switch_source(&mut self, id: &SourceId) -> bool {
        if !self.sources.iter().any(|source| &source.id() == id) {
            return false;
        }
        if id.provider == ProviderKind::OpenSubsonic {
            return self.activate_server(&id.profile_id);
        }
        self.active_source = Some(id.clone());
        true
    }
}

#[cfg(test)]
mod tests;
