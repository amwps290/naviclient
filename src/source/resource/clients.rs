//! Bounded connection pools for header-bearing or routed audio requests.
//! Headers remain per request; only the route and redirect policy belong to a pool.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Result};
use url::{Origin, Url};

use super::timing::{CONNECT_TIMEOUT, STREAM_TIMEOUT};
use super::HttpResource;
use crate::models::network::{normalize_proxy, NetworkRoute};

const MAX_CLIENTS: usize = 16;

#[derive(PartialEq, Eq)]
struct ClientKey {
    origin: Origin,
    route: NetworkRoute,
    header_bound: bool,
}

type ClientCache<T> = Mutex<VecDeque<(ClientKey, T)>>;

fn cached<T: Clone>(
    cache: &ClientCache<T>,
    resource: &HttpResource,
    build: impl FnOnce(Origin) -> Result<T>,
) -> Result<T> {
    let key = ClientKey {
        origin: Url::parse(resource.url())
            .map_err(|_| anyhow!("invalid audio resource address"))?
            .origin(),
        route: resource.route().clone(),
        header_bound: !resource.headers().is_empty(),
    };
    if let Some(client) = lookup(cache, &key) {
        return Ok(client);
    }
    // Blocking-client construction starts a runtime thread. Neither that work
    // nor dropping a duplicate/evicted runtime may hold the pool lock.
    let client = build(key.origin.clone())?;
    let mut entries = cache.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(index) = entries.iter().position(|(existing, _)| existing == &key) {
        let entry = entries.remove(index).expect("matched client");
        let existing = entry.1.clone();
        entries.push_back(entry);
        drop(entries);
        return Ok(existing);
    }
    entries.push_back((key, client.clone()));
    let evicted = (entries.len() > MAX_CLIENTS).then(|| entries.pop_front());
    drop(entries);
    drop(evicted);
    Ok(client)
}

fn lookup<T: Clone>(cache: &ClientCache<T>, key: &ClientKey) -> Option<T> {
    let mut entries = cache.lock().unwrap_or_else(|error| error.into_inner());
    let index = entries.iter().position(|(existing, _)| existing == key)?;
    let entry = entries.remove(index).expect("matched client");
    let client = entry.1.clone();
    entries.push_back(entry);
    Some(client)
}

fn redirects(resource: &HttpResource, origin: Origin) -> reqwest::redirect::Policy {
    // Public Subsonic streams may redirect to a CDN. Header-bearing resources
    // bind their request metadata to the original origin instead.
    if resource.headers().is_empty() {
        return reqwest::redirect::Policy::limited(10);
    }
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() < 10 && attempt.url().origin() == origin {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

/// Apply a source route to either reqwest builder. `System` keeps reqwest's
/// platform/environment proxy discovery; the other routes disable it. Proxy
/// addresses are re-validated, so no client embeds proxy credentials.
macro_rules! apply_route {
    ($builder:expr, $route:expr) => {
        match $route {
            NetworkRoute::System => Ok($builder),
            NetworkRoute::Direct => Ok($builder.no_proxy()),
            NetworkRoute::Proxy(url) => normalize_proxy(url)
                .ok()
                .and_then(|url| reqwest::Proxy::all(url).ok())
                .map(|proxy| $builder.no_proxy().proxy(proxy))
                .ok_or_else(|| anyhow!("invalid proxy address")),
        }
    };
}

pub(crate) fn route_async(
    builder: reqwest::ClientBuilder,
    route: &NetworkRoute,
) -> Result<reqwest::ClientBuilder> {
    apply_route!(builder, route)
}

pub(crate) fn route_blocking(
    builder: reqwest::blocking::ClientBuilder,
    route: &NetworkRoute,
) -> Result<reqwest::blocking::ClientBuilder> {
    apply_route!(builder, route)
}

/// The blocking and async clients share every setting except their type.
macro_rules! resource_client {
    ($client:ty, $route:ident, $resource:expr, $origin:expr) => {{
        let builder = <$client>::builder()
            .referer(false)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(STREAM_TIMEOUT)
            .redirect(redirects($resource, $origin));
        $route(builder, $resource.route())?
            .build()
            .map_err(reqwest::Error::without_url)
            .map_err(Into::into)
    }};
}

pub(crate) fn blocking(resource: &HttpResource) -> Result<reqwest::blocking::Client> {
    static CLIENTS: OnceLock<ClientCache<reqwest::blocking::Client>> = OnceLock::new();
    blocking_cached(resource, CLIENTS.get_or_init(Mutex::default))
}

fn blocking_cached(
    resource: &HttpResource,
    cache: &ClientCache<reqwest::blocking::Client>,
) -> Result<reqwest::blocking::Client> {
    cached(cache, resource, |origin| {
        resource_client!(reqwest::blocking::Client, route_blocking, resource, origin)
    })
}

/// Owned by a Bilibili client and shared by its clones, within that client's
/// runtime lifetime. Independent clients/runtimes never share idle connections.
#[derive(Clone, Default)]
pub(crate) struct AsyncClients {
    cache: Arc<ClientCache<reqwest::Client>>,
}

impl AsyncClients {
    pub(crate) fn get(&self, resource: &HttpResource) -> Result<reqwest::Client> {
        asynchronous_cached(resource, &self.cache)
    }
}

fn asynchronous_cached(
    resource: &HttpResource,
    cache: &ClientCache<reqwest::Client>,
) -> Result<reqwest::Client> {
    cached(cache, resource, |origin| {
        resource_client!(reqwest::Client, route_async, resource, origin)
    })
}

#[cfg(test)]
mod tests;
