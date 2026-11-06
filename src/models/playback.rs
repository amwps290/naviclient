use rust_i18n::t;
use serde::{Deserialize, Serialize};

/// One runtime playback occurrence, retained by retries and seeks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackId(u128);

impl PlaybackId {
    pub(crate) fn new() -> Self {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            OnceLock,
        };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        static PROCESS: OnceLock<u64> = OnceLock::new();
        let process = *PROCESS.get_or_init(rand::random);
        Self((u128::from(process) << 64) | u128::from(NEXT.fetch_add(1, Ordering::Relaxed)))
    }

    pub(crate) fn bytes(self) -> [u8; 16] {
        self.0.to_le_bytes()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscodingQuality {
    #[default]
    Original,
    Kbps128,
    Kbps192,
    Kbps256,
    Kbps320,
}

impl TranscodingQuality {
    pub const ALL: [Self; 5] = [
        Self::Original,
        Self::Kbps128,
        Self::Kbps192,
        Self::Kbps256,
        Self::Kbps320,
    ];

    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Original => t!("Original"),
            Self::Kbps128 => t!("128 kbps"),
            Self::Kbps192 => t!("192 kbps"),
            Self::Kbps256 => t!("256 kbps"),
            Self::Kbps320 => t!("320 kbps"),
        }
    }

    pub fn max_bit_rate(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::Kbps128 => Some(128),
            Self::Kbps192 => Some(192),
            Self::Kbps256 => Some(256),
            Self::Kbps320 => Some(320),
        }
    }

    pub fn cache_profile(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Kbps128 => "mp3-128",
            Self::Kbps192 => "mp3-192",
            Self::Kbps256 => "mp3-256",
            Self::Kbps320 => "mp3-320",
        }
    }

    pub fn is_undecodable_source(suffix: Option<&str>, content_type: Option<&str>) -> bool {
        let is = |text: Option<&str>, expected: &str| {
            text.is_some_and(|text| text.trim().eq_ignore_ascii_case(expected))
        };
        is(suffix, "opus") || is(content_type, "audio/opus")
    }

    pub fn effective_for(
        configured: Self,
        suffix: Option<&str>,
        content_type: Option<&str>,
    ) -> Self {
        if configured == Self::Original && Self::is_undecodable_source(suffix, content_type) {
            Self::transcode_fallback()
        } else {
            configured
        }
    }

    pub const fn transcode_fallback() -> Self {
        Self::Kbps320
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    #[default]
    Sequential,
    RepeatAll,
    RepeatOne,
    Shuffle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn tooltip(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Off => t!("Repeat off"),
            Self::All => t!("Repeat all"),
            Self::One => t!("Repeat current track"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlayModes {
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub repeat: RepeatMode,
}

impl PlayModes {
    pub fn from_legacy(mode: PlaybackMode) -> Self {
        match mode {
            PlaybackMode::Sequential => Self {
                shuffle: false,
                repeat: RepeatMode::Off,
            },
            PlaybackMode::RepeatAll => Self {
                shuffle: false,
                repeat: RepeatMode::All,
            },
            PlaybackMode::RepeatOne => Self {
                shuffle: false,
                repeat: RepeatMode::One,
            },
            PlaybackMode::Shuffle => Self {
                shuffle: true,
                repeat: RepeatMode::Off,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeNormalization {
    Off,
    #[default]
    Track,
    Album,
}

impl VolumeNormalization {
    pub const ALL: [Self; 3] = [Self::Off, Self::Track, Self::Album];

    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Off => t!("Off"),
            Self::Track => t!("Track"),
            Self::Album => t!("Album"),
        }
    }

    pub fn tooltip(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Off => t!("Play every song at its recorded loudness"),
            Self::Track => t!("Normalize each track to a consistent loudness (ReplayGain)"),
            Self::Album => t!("Normalize by album so a record's relative dynamics are preserved"),
        }
    }
}
