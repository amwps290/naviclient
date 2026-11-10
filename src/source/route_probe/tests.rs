use super::*;
use crate::test_support::http::Server;
use std::net::TcpListener;

#[tokio::test]
async fn a_relayed_response_reports_latency_even_when_the_target_status_is_not_success() {
    for status in [204, 503] {
        let proxy = Server::new(vec![(status, String::new())]);
        let route = NetworkRoute::Proxy(proxy.url("").trim_end_matches('/').into());
        let result = probe_route_to(
            &route,
            "http://probe.invalid/generate_204",
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        assert!(result < Duration::from_secs(3));
        let requests = proxy.requests();
        assert_eq!(requests.len(), 1);
        // The proxy, not the local resolver, handled the unreachable host.
        assert_eq!(requests[0].url.host_str(), Some("probe.invalid"));
    }
}

#[tokio::test]
async fn a_silent_proxy_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let route = NetworkRoute::Proxy(format!("http://{}", listener.local_addr().unwrap()));
    assert_eq!(
        probe_route_to(&route, "http://probe.invalid/", Duration::from_millis(300)).await,
        Err(RouteProbeError::Timeout)
    );
    drop(listener);
}

#[tokio::test]
async fn invalid_proxy_addresses_are_rejected_before_connecting() {
    let route = NetworkRoute::Proxy("http://user:secret@127.0.0.1:1".into());
    assert_eq!(
        probe_route_to(&route, "http://probe.invalid/", Duration::from_secs(1)).await,
        Err(RouteProbeError::InvalidProxy)
    );
}
