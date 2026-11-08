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
        expires_at: None,
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
    refreshed.expires_at = Some(SystemTime::now());
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
            .with_proxy(Some(
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
