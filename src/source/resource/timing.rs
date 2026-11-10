//! Playback has two sequential network stages: resolving a representation,
//! then opening its stream. The outer UI timeout leaves room for a provider to
//! report its last CDN failure. These are not a single total startup deadline.

use std::time::Duration;

pub(crate) const AUDIO_PROBE_TIMEOUT: Duration = Duration::from_secs(6);
pub(crate) const STREAM_TIMEOUT: Duration = Duration::from_secs(12);
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const RESOLUTION_BUDGET: Duration = Duration::from_secs(25);
pub(crate) const INITIAL_CONNECTION_BUDGET: Duration = Duration::from_secs(25);
pub(crate) const START_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Avoid selecting addresses that would expire during stream preparation.
pub(crate) const EXPIRY_MARGIN: Duration = Duration::from_secs(30);
