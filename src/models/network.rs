//! Shared proxy definitions and per-source routing, separate from authentication.

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::{
    source::{ProviderKind, SourceId, SourceProfile},
    Config,
};

/// Version 1 introduced the explicit system-proxy route. Older files are
/// migrated so every source keeps the route it used before the upgrade.
const NETWORK_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSettings {
    /// Missing in files written before routing was versioned.
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub proxies: Vec<ProxyProfile>,
    #[serde(default)]
    pub source_proxies: Vec<SourceProxy>,
}

impl Default for NetworkSettings {
    /// A new configuration has nothing to migrate.
    fn default() -> Self {
        Self {
            version: NETWORK_VERSION,
            ..Self::legacy()
        }
    }
}

impl NetworkSettings {
    /// Settings absent from a saved file predate routing and must migrate.
    pub fn legacy() -> Self {
        Self {
            version: 0,
            proxies: Vec::new(),
            source_proxies: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyProfile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub url: String,
}

impl ProxyProfile {
    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.url
        } else {
            self.name.trim()
        }
    }
}

/// The connection a user chose for one source. Missing entries mean `System`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConnection {
    #[default]
    System,
    Direct,
    Proxy(String),
}

/// A resolved transport for building HTTP clients.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum NetworkRoute {
    /// The platform/environment proxy that reqwest discovers by default.
    #[default]
    System,
    Direct,
    Proxy(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SourceProxyWire", into = "SourceProxyWire")]
pub struct SourceProxy {
    pub source: SourceId,
    pub connection: SourceConnection,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RouteKind {
    System,
    Direct,
    /// Entries written before the route field existed held only `proxy_id`.
    #[default]
    Proxy,
}

#[derive(Serialize, Deserialize)]
struct SourceProxyWire {
    source: SourceId,
    #[serde(default)]
    route: RouteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proxy_id: Option<String>,
}

impl From<SourceProxyWire> for SourceProxy {
    fn from(wire: SourceProxyWire) -> Self {
        let connection = match (wire.route, wire.proxy_id) {
            (RouteKind::Direct, _) => SourceConnection::Direct,
            (RouteKind::Proxy, Some(id)) => SourceConnection::Proxy(id),
            (RouteKind::System | RouteKind::Proxy, _) => SourceConnection::System,
        };
        Self {
            source: wire.source,
            connection,
        }
    }
}

impl From<SourceProxy> for SourceProxyWire {
    fn from(entry: SourceProxy) -> Self {
        let (route, proxy_id) = match entry.connection {
            SourceConnection::System => (RouteKind::System, None),
            SourceConnection::Direct => (RouteKind::Direct, None),
            SourceConnection::Proxy(id) => (RouteKind::Proxy, Some(id)),
        };
        Self {
            source: entry.source,
            route,
            proxy_id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkError {
    InvalidProxy,
    MissingProxy,
    MissingSource,
    ProxyInUse,
}

/// Normalize an HTTP(S)/SOCKS5 proxy without connecting or storing credentials.
/// A bare `host:port` means HTTP, matching how proxy tools print addresses.
pub fn normalize_proxy(input: &str) -> Result<String, NetworkError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(NetworkError::InvalidProxy);
    }
    let input = if input.contains("://") {
        input.to_string()
    } else {
        format!("http://{input}")
    };
    let parsed = url::Url::parse(&input).map_err(|_| NetworkError::InvalidProxy)?;
    if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h")
        || parsed.host_str().is_none_or(str::is_empty)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(NetworkError::InvalidProxy);
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

impl Config {
    pub fn source_connection(&self, source: &SourceId) -> SourceConnection {
        self.network
            .source_proxies
            .iter()
            .find(|entry| &entry.source == source)
            .map(|entry| entry.connection.clone())
            .unwrap_or_default()
    }

    /// A broken reference is an error, so routing never silently changes.
    pub fn route_for(&self, source: &SourceId) -> Result<NetworkRoute, NetworkError> {
        self.resolve_connection(&self.source_connection(source))
    }

    pub fn resolve_connection(
        &self,
        connection: &SourceConnection,
    ) -> Result<NetworkRoute, NetworkError> {
        match connection {
            SourceConnection::System => Ok(NetworkRoute::System),
            SourceConnection::Direct => Ok(NetworkRoute::Direct),
            SourceConnection::Proxy(id) => self
                .network
                .proxies
                .iter()
                .find(|proxy| &proxy.id == id)
                .map(|proxy| NetworkRoute::Proxy(proxy.url.clone()))
                .ok_or(NetworkError::MissingProxy),
        }
    }

    /// How many sources currently route through this proxy.
    pub fn proxy_users(&self, id: &str) -> usize {
        usize::from(self.lyrics.connection == SourceConnection::Proxy(id.to_string())) + self.network
            .source_proxies
            .iter()
            .filter(
                |entry| matches!(&entry.connection, SourceConnection::Proxy(used) if used == id),
            )
            .count()
    }

    pub fn store_proxy(
        &mut self,
        id: Option<&str>,
        name: &str,
        input: &str,
    ) -> Result<String, NetworkError> {
        let url = normalize_proxy(input)?;
        if let Some(id) = id {
            let proxy = self
                .network
                .proxies
                .iter_mut()
                .find(|proxy| proxy.id == id)
                .ok_or(NetworkError::MissingProxy)?;
            proxy.name = name.trim().into();
            proxy.url = url;
            return Ok(id.into());
        }
        let id = format!("{:032x}", rand::random::<u128>());
        self.network.proxies.push(ProxyProfile {
            id: id.clone(),
            name: name.trim().into(),
            url,
        });
        Ok(id)
    }

    pub fn set_source_connection(
        &mut self,
        source: &SourceId,
        connection: SourceConnection,
    ) -> Result<(), NetworkError> {
        if !self.sources.iter().any(|profile| &profile.id() == source) {
            return Err(NetworkError::MissingSource);
        }
        if let SourceConnection::Proxy(id) = &connection {
            if !self.network.proxies.iter().any(|proxy| &proxy.id == id) {
                return Err(NetworkError::MissingProxy);
            }
        }
        self.network
            .source_proxies
            .retain(|entry| &entry.source != source);
        if connection != SourceConnection::System {
            self.network.source_proxies.push(SourceProxy {
                source: source.clone(),
                connection,
            });
        }
        if let Some(SourceProfile::Bilibili { proxy, .. }) = self
            .sources
            .iter_mut()
            .find(|profile| &profile.id() == source)
        {
            *proxy = None;
        }
        Ok(())
    }

    pub fn remove_proxy(&mut self, id: &str) -> Result<(), NetworkError> {
        if self.proxy_users(id) > 0 {
            return Err(NetworkError::ProxyInUse);
        }
        if !self.network.proxies.iter().any(|proxy| proxy.id == id) {
            return Err(NetworkError::MissingProxy);
        }
        self.network.proxies.retain(|proxy| proxy.id != id);
        Ok(())
    }

    pub fn sync_network(&mut self) {
        // One shared definition per legacy address, retaining distinct addresses.
        let legacy: Vec<_> = self
            .sources
            .iter_mut()
            .filter_map(|profile| {
                let source = profile.id();
                match profile {
                    SourceProfile::Bilibili { proxy, .. } => proxy.take().map(|url| (source, url)),
                    _ => None,
                }
            })
            .collect();
        for (source, url) in legacy {
            let Ok(url) = normalize_proxy(&url) else {
                continue;
            };
            self.network
                .source_proxies
                .retain(|entry| entry.source != source);
            let id = if let Some(proxy) = self.network.proxies.iter().find(|proxy| proxy.url == url)
            {
                proxy.id.clone()
            } else {
                let mut id = format!("proxy-{:x}", Md5::digest(url.as_bytes()));
                while self.network.proxies.iter().any(|proxy| proxy.id == id) {
                    id.push('-');
                }
                self.network.proxies.push(ProxyProfile {
                    id: id.clone(),
                    name: String::new(),
                    url,
                });
                id
            };
            self.network.source_proxies.push(SourceProxy {
                source,
                connection: SourceConnection::Proxy(id),
            });
        }
        if self.network.version < NETWORK_VERSION {
            // Before version 1 a Bilibili source without a proxy was direct.
            let direct: Vec<_> = self
                .sources
                .iter()
                .map(SourceProfile::id)
                .filter(|source| {
                    source.provider == ProviderKind::Bilibili
                        && !self
                            .network
                            .source_proxies
                            .iter()
                            .any(|entry| &entry.source == source)
                })
                .collect();
            for source in direct {
                self.network.source_proxies.push(SourceProxy {
                    source,
                    connection: SourceConnection::Direct,
                });
            }
            self.network.version = NETWORK_VERSION;
        }
        self.network.source_proxies.retain(|entry| {
            self.sources
                .iter()
                .any(|profile| profile.id() == entry.source)
        });
    }
}

#[cfg(test)]
mod tests;
