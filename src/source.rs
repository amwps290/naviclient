//! Provider dispatch and resolved media, independent of the application UI.

pub mod bilibili;
pub mod local;
mod opensubsonic;
pub(crate) mod playback;
mod queries;
pub mod resource;
pub(crate) mod route_probe;

pub use crate::models::source::{
    MediaKey, PlaybackPolicy, ProviderKind, SourceCapabilities, SourceId, SourceProfile,
};

pub use opensubsonic::OpenSubsonicAdapter;
pub use resource::{AudioQuality, CachePolicy, HttpResource, PlaybackResource};

use crate::models::network::NetworkRoute;
use crate::models::{Song, TranscodingQuality};
use anyhow::Result;

#[derive(Clone)]
pub enum MusicSource {
    OpenSubsonic(OpenSubsonicAdapter),
    Bilibili(bilibili::BilibiliProvider),
    LocalFolder(local::LocalFolderProvider),
}

impl MusicSource {
    pub fn opensubsonic(
        profile_id: &str,
        server_url: &str,
        username: &str,
        password: &str,
    ) -> Result<Self> {
        Ok(Self::OpenSubsonic(OpenSubsonicAdapter::new(
            profile_id, server_url, username, password,
        )?))
    }

    pub fn local_folder(
        profile_id: impl Into<String>,
        name: impl Into<String>,
        roots: Vec<std::path::PathBuf>,
    ) -> Self {
        Self::LocalFolder(local::LocalFolderProvider::new(
            SourceId::new(ProviderKind::LocalFolder, profile_id),
            name.into(),
            roots,
        ))
    }

    pub fn id(&self) -> &SourceId {
        match self {
            Self::OpenSubsonic(adapter) => adapter.id(),
            Self::Bilibili(provider) => provider.id(),
            Self::LocalFolder(provider) => provider.id(),
        }
    }

    pub fn opensubsonic_with_route(
        profile_id: &str,
        server_url: &str,
        username: &str,
        password: &str,
        route: NetworkRoute,
    ) -> Result<Self> {
        Ok(Self::OpenSubsonic(OpenSubsonicAdapter::with_route(
            profile_id, server_url, username, password, route,
        )?))
    }

    pub fn capabilities(&self) -> SourceCapabilities {
        self.id().provider.capabilities()
    }

    pub fn playback_policy(&self) -> PlaybackPolicy {
        self.id().provider.playback_policy()
    }

    /// Transitional address for legacy server/session APIs. Media identity
    /// and new resource cache keys use `id()` instead of this address.
    pub fn base_url(&self) -> &str {
        match self {
            Self::OpenSubsonic(adapter) => adapter.provider().base_url(),
            Self::Bilibili(_) => "https://www.bilibili.com",
            Self::LocalFolder(_) => "local://folder",
        }
    }

    pub async fn resolve_playback(
        &self,
        song: &Song,
        configured: TranscodingQuality,
    ) -> Result<PlaybackResource> {
        match self {
            Self::OpenSubsonic(_) | Self::LocalFolder(_) => self.resolve_immediate(song, configured),
            Self::Bilibili(provider) => Ok(provider.resolve_playback(song).await?),
        }
    }

    /// Providers that require a request return `None`; the application then
    /// resolves asynchronously while the current track continues to play.
    pub(crate) fn try_resolve_immediate(
        &self,
        song: &Song,
        configured: TranscodingQuality,
    ) -> Option<Result<PlaybackResource>> {
        match self {
            Self::OpenSubsonic(_) | Self::LocalFolder(_) => {
                Some(self.resolve_immediate(song, configured))
            }
            Self::Bilibili(_) => None,
        }
    }

    /// Existing OpenSubsonic URL construction needs no network I/O. This fast
    /// path keeps playback transactions synchronous during the migration;
    /// providers requiring I/O will use `resolve_playback` with cancellation.
    pub(crate) fn resolve_immediate(
        &self,
        song: &Song,
        configured: TranscodingQuality,
    ) -> Result<PlaybackResource> {
        match self {
            Self::OpenSubsonic(adapter) => adapter.resolve_playback(song, configured),
            Self::LocalFolder(provider) => provider.resolve_playback(song),
            Self::Bilibili(_) => anyhow::bail!("Bilibili audio must be resolved asynchronously"),
        }
    }
}

#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod tests;
