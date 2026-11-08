//! Provider identity and source profiles, independent of transport credentials.

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    OpenSubsonic,
    Bilibili,
}

/// The provider is part of identity, so equal profile IDs cannot cross sources.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId {
    #[serde(default)]
    pub provider: ProviderKind,
    pub profile_id: String,
}

impl SourceId {
    pub fn new(provider: ProviderKind, profile_id: impl Into<String>) -> Self {
        Self {
            provider,
            profile_id: profile_id.into(),
        }
    }

    /// A stable, path-safe namespace for new source data. Legacy OpenSubsonic
    /// paths are retained by the persistence adapter instead of renamed here.
    pub fn cache_namespace(&self) -> String {
        format!("source-{:032x}", u128::from_be_bytes(self.fingerprint()))
    }

    /// Compact source ownership for copyable UI event targets.
    pub(crate) fn fingerprint(&self) -> [u8; 16] {
        let mut hash = Md5::new();
        self.hash_into(&mut hash);
        hash.finalize().into()
    }

    fn hash_into(&self, hash: &mut Md5) {
        hash.update([match self.provider {
            ProviderKind::OpenSubsonic => 0,
            ProviderKind::Bilibili => 1,
        }]);
        hash.update((self.profile_id.len() as u64).to_le_bytes());
        hash.update(self.profile_id.as_bytes());
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaKey {
    pub source: SourceId,
    pub remote_id: String,
}

impl MediaKey {
    pub fn new(source: SourceId, remote_id: impl Into<String>) -> Self {
        Self {
            source,
            remote_id: remote_id.into(),
        }
    }

    /// Resource URLs, titles and Cookie values are deliberately absent.
    pub fn cache_key(&self) -> String {
        let mut hash = Md5::new();
        self.source.hash_into(&mut hash);
        hash.update((self.remote_id.len() as u64).to_le_bytes());
        hash.update(self.remote_id.as_bytes());
        format!("{:x}", hash.finalize())
    }
}

/// OpenSubsonic references its existing profile: there is only one copy of
/// server credentials to migrate. Bilibili stores a credential-store reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum SourceProfile {
    OpenSubsonic {
        server_id: String,
    },
    Bilibili {
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        account_id: Option<u64>,
        #[serde(default)]
        credential_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proxy: Option<String>,
    },
}

impl SourceProfile {
    pub fn id(&self) -> SourceId {
        match self {
            Self::OpenSubsonic { server_id } => {
                SourceId::new(ProviderKind::OpenSubsonic, server_id)
            }
            Self::Bilibili { id, .. } => SourceId::new(ProviderKind::Bilibili, id),
        }
    }
}

/// Supported provider operations. Account permissions and negotiated server
/// extensions remain separate runtime facts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceCapabilities {
    pub library: bool,
    pub search: bool,
    pub favorite_read: bool,
    pub favorite_write: bool,
    pub playlist_read: bool,
    pub playlist_write: bool,
    pub lyrics: bool,
    pub recommendations: bool,
    pub scrobble: bool,
    pub transcoding: bool,
}

impl ProviderKind {
    pub fn capabilities(self) -> SourceCapabilities {
        match self {
            Self::OpenSubsonic => SourceCapabilities {
                library: true,
                search: true,
                favorite_read: true,
                favorite_write: true,
                playlist_read: true,
                playlist_write: true,
                lyrics: true,
                recommendations: true,
                scrobble: true,
                transcoding: true,
            },
            Self::Bilibili => SourceCapabilities {
                favorite_read: true,
                playlist_read: true,
                ..SourceCapabilities::default()
            },
        }
    }
}

#[cfg(test)]
mod tests;
