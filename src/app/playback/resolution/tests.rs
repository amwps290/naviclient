use super::*;
use crate::source::bilibili::client::BilibiliError;

#[test]
fn transport_failures_retain_retry_classification_without_retrying_risk_control() {
    for (error, expected) in [
        (BilibiliError::Network, Some(StreamFailure::Connect)),
        (BilibiliError::Timeout, Some(StreamFailure::Timeout)),
        (BilibiliError::Http(503), Some(StreamFailure::Http(503))),
        (BilibiliError::Http(408), Some(StreamFailure::Http(408))),
        (BilibiliError::RateLimited, None),
        (BilibiliError::Http(429), None),
        (BilibiliError::Http(403), None),
        (BilibiliError::NotAuthenticated, None),
        (BilibiliError::PermissionDenied, None),
        (BilibiliError::UnsupportedAudio, None),
        (BilibiliError::ResourceExpired, None),
        (BilibiliError::InvalidResponse, None),
    ] {
        let result = resolution_error(error.into());
        assert_eq!(result.retry_failure, expected, "{error:?}");
        assert_eq!(
            result.login_expired,
            error == BilibiliError::NotAuthenticated
        );
    }
}

#[test]
fn resolution_failures_do_not_confuse_network_errors_with_unavailable_items() {
    for error in [
        BilibiliError::Unavailable,
        BilibiliError::PermissionDenied,
        BilibiliError::RegionRestricted,
        BilibiliError::UnsupportedAudio,
    ] {
        let result = resolution_error(error.into());
        assert!(result.unavailable);
        assert!(result.retry_failure.is_none());
    }
    assert!(!resolution_error(BilibiliError::Network.into()).unavailable);
    assert!(!resolution_error(BilibiliError::ResourceExpired.into()).unavailable);
    assert!(
        resolution_error(anyhow::anyhow!("invalid resource identity"))
            .retry_failure
            .is_none()
    );
}
