//! Provider errors are translated into playback decisions at the source boundary.

use super::bilibili::client::BilibiliError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolutionFailure {
    Network,
    Timeout,
    Http(u16),
    SessionExpired,
    Unavailable,
    Other,
}

pub(crate) fn classify_resolution_error(error: &anyhow::Error) -> ResolutionFailure {
    match error.downcast_ref::<BilibiliError>() {
        Some(BilibiliError::Network) => ResolutionFailure::Network,
        Some(BilibiliError::Timeout) => ResolutionFailure::Timeout,
        Some(BilibiliError::Http(status)) => ResolutionFailure::Http(*status),
        Some(BilibiliError::NotAuthenticated) => ResolutionFailure::SessionExpired,
        Some(
            BilibiliError::Unavailable
            | BilibiliError::PermissionDenied
            | BilibiliError::RegionRestricted
            | BilibiliError::UnsupportedAudio,
        ) => ResolutionFailure::Unavailable,
        // Expired addresses, risk control, malformed responses and local
        // preparation errors are not evidence that the media is unavailable.
        _ => ResolutionFailure::Other,
    }
}
