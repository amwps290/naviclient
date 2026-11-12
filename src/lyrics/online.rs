//! Statically dispatched online providers with stable identities.

use super::{
    kugou::Kugou,
    lrclib::Lrclib,
    netease::Netease,
    provider::{LyricsProvider, ProviderError, ProviderId, Query, Record},
    transport::RequestGate,
};
use crate::models::network::NetworkRoute;

#[derive(Clone)]
pub(crate) enum Provider {
    Lrclib(Lrclib),
    Kugou(Kugou),
    Netease(Netease),
}

impl Provider {
    pub fn new(
        id: ProviderId,
        route: &NetworkRoute,
        gate: RequestGate,
    ) -> Result<Self, ProviderError> {
        Ok(match id {
            ProviderId::Lrclib => Self::Lrclib(Lrclib::new(route, gate)?),
            ProviderId::Kugou => Self::Kugou(Kugou::new(route, gate)?),
            ProviderId::Netease => Self::Netease(Netease::new(route, gate)?),
        })
    }
    pub fn id(&self) -> ProviderId {
        match self {
            Self::Lrclib(_) => ProviderId::Lrclib,
            Self::Kugou(_) => ProviderId::Kugou,
            Self::Netease(_) => ProviderId::Netease,
        }
    }
}

impl LyricsProvider for Provider {
    async fn lookup(&self, query: &Query) -> Result<Option<Record>, ProviderError> {
        match self {
            Self::Lrclib(p) => p.lookup(query).await,
            Self::Kugou(p) => p.lookup(query).await,
            Self::Netease(p) => p.lookup(query).await,
        }
    }
    async fn search(&self, query: &Query) -> Result<Vec<Record>, ProviderError> {
        match self {
            Self::Lrclib(p) => p.search(query).await,
            Self::Kugou(p) => p.search(query).await,
            Self::Netease(p) => p.search(query).await,
        }
    }
    async fn fetch(&self, id: u64, query: Option<&Query>) -> Result<Option<Record>, ProviderError> {
        match self {
            Self::Lrclib(p) => p.fetch(id, query).await,
            Self::Kugou(p) => p.fetch(id, query).await,
            Self::Netease(p) => p.fetch(id, query).await,
        }
    }
}
