#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenSubsonicExtension {
    pub name: String,
    pub versions: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CapabilitySupport {
    #[default]
    Unknown,
    Unsupported,
    Supported,
}

impl CapabilitySupport {
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }

    /// Compatibility mode keeps established fallback requests available when
    /// the server cannot advertise extensions. Only an explicit unsupported
    /// result should suppress an existing request path.
    pub const fn allows_optimistic_request(self) -> bool {
        !matches!(self, Self::Unsupported)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerCapabilities {
    pub structured_lyrics: CapabilitySupport,
    pub enhanced_lyrics: CapabilitySupport,
    pub sonic_similarity: CapabilitySupport,
    pub top_songs_by_artist_id: CapabilitySupport,
    pub playback_report: CapabilitySupport,
    pub index_based_queue: CapabilitySupport,
    pub advanced_transcoding: CapabilitySupport,
    pub transcode_offset: CapabilitySupport,
    pub internet_radio: CapabilitySupport,
    pub podcasts: CapabilitySupport,
    pub podcast_episode_lookup: CapabilitySupport,
}

impl ServerCapabilities {
    pub fn from_server_info(info: &ServerInfo) -> Self {
        let extension = |name, minimum_version| {
            if !info.extensions_discovered {
                CapabilitySupport::Unknown
            } else if info.supports_extension(name, minimum_version) {
                CapabilitySupport::Supported
            } else {
                CapabilitySupport::Unsupported
            }
        };

        Self {
            structured_lyrics: extension("songLyrics", 1),
            enhanced_lyrics: extension("songLyrics", 2),
            sonic_similarity: extension("sonicSimilarity", 1),
            top_songs_by_artist_id: extension("topSongsByArtistId", 1),
            playback_report: extension("playbackReport", 1),
            index_based_queue: extension("indexBasedQueue", 1),
            advanced_transcoding: extension("transcoding", 1),
            transcode_offset: extension("transcodeOffset", 1),
            internet_radio: protocol_support(&info.protocol_version, [1, 9, 0]),
            podcasts: protocol_support(&info.protocol_version, [1, 6, 0]),
            podcast_episode_lookup: extension("getPodcastEpisode", 1),
        }
    }

    pub const fn smart_discovery(self) -> CapabilitySupport {
        if self.sonic_similarity.is_supported() || self.top_songs_by_artist_id.is_supported() {
            CapabilitySupport::Supported
        } else if matches!(self.sonic_similarity, CapabilitySupport::Unsupported)
            && matches!(self.top_songs_by_artist_id, CapabilitySupport::Unsupported)
        {
            CapabilitySupport::Unsupported
        } else {
            CapabilitySupport::Unknown
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// 服务端实现名称，例如 Navidrome、gonic 或 Ampache。
    pub app_name: Option<String>,
    /// 服务端自身版本；旧服务端可能只返回协议版本。
    pub server_version: Option<String>,
    /// 服务端声明的 Subsonic/OpenSubsonic 协议版本。
    pub protocol_version: String,
    pub open_subsonic: bool,
    /// `getOpenSubsonicExtensions` completed successfully. This is separate
    /// from `open_subsonic`: ping may advertise OpenSubsonic before the
    /// extension probe finishes or when that optional probe later fails.
    pub extensions_discovered: bool,
    pub extensions: Vec<OpenSubsonicExtension>,
    pub capabilities: ServerCapabilities,
}

impl ServerInfo {
    pub fn display_version(&self) -> &str {
        self.server_version
            .as_deref()
            .unwrap_or(self.protocol_version.as_str())
    }

    pub fn supports_extension(&self, name: &str, minimum_version: u32) -> bool {
        self.extensions.iter().any(|extension| {
            extension.name.eq_ignore_ascii_case(name)
                && extension
                    .versions
                    .iter()
                    .any(|version| *version >= minimum_version)
        })
    }

    pub fn merge_open_subsonic_info(&mut self, discovered: ServerInfo) {
        if discovered.app_name.is_some() {
            self.app_name = discovered.app_name;
        }
        if discovered.server_version.is_some() {
            self.server_version = discovered.server_version;
        }
        if discovered.protocol_version != "unknown" {
            self.protocol_version = discovered.protocol_version;
        }
        self.open_subsonic = discovered.open_subsonic;
        self.extensions_discovered = discovered.extensions_discovered;
        self.extensions = discovered.extensions;
        self.refresh_capabilities();
    }

    pub fn refresh_capabilities(&mut self) {
        self.capabilities = ServerCapabilities::from_server_info(self);
    }

    pub fn extension_summary(&self, limit: usize) -> Option<String> {
        if self.extensions.is_empty() || limit == 0 {
            return None;
        }
        let mut labels = self
            .extensions
            .iter()
            .take(limit)
            .map(|extension| {
                if extension.versions.is_empty() {
                    extension.name.clone()
                } else {
                    format!(
                        "{} v{}",
                        extension.name,
                        extension
                            .versions
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join("/")
                    )
                }
            })
            .collect::<Vec<_>>();
        let remaining = self.extensions.len().saturating_sub(labels.len());
        if remaining > 0 {
            labels.push(format!("+{remaining}"));
        }
        Some(labels.join(", "))
    }
}

fn protocol_support(version: &str, minimum: [u32; 3]) -> CapabilitySupport {
    let mut parts = version.split('.');
    let parsed = [
        parts.next().and_then(|part| part.parse::<u32>().ok()),
        parts.next().and_then(|part| part.parse::<u32>().ok()),
        parts.next().and_then(|part| part.parse::<u32>().ok()),
    ];
    let [Some(major), Some(minor), Some(patch)] = parsed else {
        return CapabilitySupport::Unknown;
    };
    if [major, minor, patch] >= minimum {
        CapabilitySupport::Supported
    } else {
        CapabilitySupport::Unsupported
    }
}
