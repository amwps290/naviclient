//! Account-scoped favorites and independent audio. Video streams are never used.

pub(crate) mod account;
pub mod auth;
pub mod client;
mod library;
mod playback;

use crate::models::network::NetworkRoute;
use crate::models::Song;
use crate::source::{PlaybackResource, ProviderKind, SourceId};
use auth::AuthSession;
use client::{BilibiliClient, BilibiliError};
pub use library::{FavoriteFolder, FolderTracks};

#[derive(Clone)]
pub struct BilibiliProvider {
    id: SourceId,
    client: BilibiliClient,
    session: AuthSession,
    route: NetworkRoute,
}

impl BilibiliProvider {
    pub fn new(
        id: SourceId,
        session: AuthSession,
        route: NetworkRoute,
    ) -> Result<Self, BilibiliError> {
        if id.provider != ProviderKind::Bilibili || id.profile_id.is_empty() {
            return Err(BilibiliError::AccountMismatch);
        }
        Ok(Self {
            id,
            client: BilibiliClient::new(&route)?,
            session,
            route,
        })
    }

    pub fn id(&self) -> &SourceId {
        &self.id
    }

    /// Renew account authorization without losing the runtime-owned CDN pool.
    pub(crate) fn with_session(
        &self,
        session: AuthSession,
        route: NetworkRoute,
    ) -> Result<Self, BilibiliError> {
        if route != self.route {
            return Self::new(self.id.clone(), session, route);
        }
        Ok(Self {
            session,
            ..self.clone()
        })
    }

    pub fn cover_url(&self, id: &str) -> Result<String, BilibiliError> {
        library::safe_artwork(id)
            .map(|url| url.to_string())
            .ok_or(BilibiliError::InvalidResponse)
    }

    pub async fn artwork(&self, url: &str) -> Result<Vec<u8>, BilibiliError> {
        self.client.artwork(url).await
    }

    pub async fn verify(&self) -> Result<auth::Account, BilibiliError> {
        self.client.verify_account(&self.session).await
    }

    pub async fn resolve_playback(&self, song: &Song) -> Result<PlaybackResource, BilibiliError> {
        playback::resolve(self, song).await
    }
}

#[cfg(test)]
mod tests;
