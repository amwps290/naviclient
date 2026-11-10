use super::*;
use crate::source::{ProviderKind, SourceId};
use reqwest::header::{HeaderValue, AUTHORIZATION};

fn resource(provider: ProviderKind, profile: &str, representation: &str) -> PlaybackResource {
    PlaybackResource {
        media: MediaKey::new(SourceId::new(provider, profile), "same-id"),
        representation: representation.into(),
        http: HttpResource::new("https://cdn.test/audio?token=secret"),
        backups: Vec::new(),
        quality: AudioQuality::default(),
        duration: None,
    }
}

#[test]
fn signed_resource_refresh_keeps_cache_identity_while_representations_and_accounts_do_not() {
    let original = resource(ProviderKind::Bilibili, "account-a", "aac-lc-30280");
    let mut refreshed = original.clone();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer new-secret"));
    refreshed.http =
        HttpResource::new("https://backup.test/refreshed?token=other").with_headers(headers);
    refreshed.http = refreshed.http.with_expiry(ResourceExpiry::from_lifetime(
        Instant::now(),
        SystemTime::now(),
        Duration::ZERO,
    ));
    assert_eq!(original.cache_key(), refreshed.cache_key());
    for other in [
        resource(ProviderKind::Bilibili, "account-b", "aac-lc-30280"),
        resource(ProviderKind::Bilibili, "account-a", "aac-lc-30232"),
        resource(ProviderKind::OpenSubsonic, "account-a", "aac-lc-30280"),
    ] {
        assert_ne!(original.cache_key(), other.cache_key());
    }
}

#[test]
fn resource_expiry_catches_sleep_and_wall_clock_rollback() {
    let received = Instant::now();
    let wall_received = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let http = HttpResource::new("https://cdn.test/audio").with_expiry(
        ResourceExpiry::from_lifetime(received, wall_received, Duration::from_secs(3600)),
    );
    assert!(!http.expired_at(received, wall_received));
    assert!(http.expired_at(received, wall_received + Duration::from_secs(3600)));
    assert!(http.expired_at(
        received + Duration::from_secs(3600),
        wall_received - Duration::from_secs(1),
    ));
    assert!(!HttpResource::new("https://cdn.test/unknown").expired_at(
        received + Duration::from_secs(7200),
        wall_received + Duration::from_secs(7200),
    ));
}

#[test]
fn debug_does_not_expose_url_headers_or_proxy_credentials() {
    let mut resource = resource(ProviderKind::Bilibili, "account", "aac");
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer header-secret"),
    );
    resource.http =
        HttpResource::new("https://url-user:url-password@cdn.test/token-in-path?token=url-secret")
            .with_headers(headers)
            .with_route(crate::models::network::NetworkRoute::Proxy(
                "http://proxy-user:proxy-secret@proxy.test:7890".into(),
            ));
    let debug = format!("{resource:?}");
    for secret in [
        "url-user",
        "url-password",
        "token-in-path",
        "url-secret",
        "header-secret",
        "proxy-user",
        "proxy-secret",
    ] {
        assert!(!debug.contains(secret));
    }
}
