//! Reachability check for a saved proxy, independent of any music source.

use std::time::{Duration, Instant};

use crate::models::network::NetworkRoute;
use crate::source::resource::clients::route_async;

/// A plain-HTTP connectivity endpoint widely used by proxy tools. Any
/// relayed response proves the proxy works, whatever its status code.
const PROBE_TARGET: &str = "http://cp.cloudflare.com/generate_204";
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteProbeError {
    InvalidProxy,
    Unreachable,
    Timeout,
}

pub(crate) async fn probe_route(route: &NetworkRoute) -> Result<Duration, RouteProbeError> {
    probe_route_to(route, PROBE_TARGET, PROBE_TIMEOUT).await
}

pub(crate) async fn probe_route_to(
    route: &NetworkRoute,
    target: &str,
    timeout: Duration,
) -> Result<Duration, RouteProbeError> {
    let client = route_async(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout),
        route,
    )
    .map_err(|_| RouteProbeError::InvalidProxy)?
    .build()
    .map_err(|_| RouteProbeError::InvalidProxy)?;
    let started = Instant::now();
    match client.get(target).send().await {
        Ok(_) => Ok(started.elapsed()),
        Err(error) if error.is_timeout() => Err(RouteProbeError::Timeout),
        Err(_) => Err(RouteProbeError::Unreachable),
    }
}

#[cfg(test)]
mod tests;
