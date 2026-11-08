mod config;
mod equalizer;
mod listening;
mod media;
mod playback;
mod preferences;
mod queue;
mod server;
mod shortcuts;
mod sort;
pub(crate) mod source;

pub use config::{Config, ServerProfile};
pub use equalizer::EqSettings;
pub use listening::{ListeningEvent, ListeningHistory};
pub use media::{
    Album, Artist, BilibiliPart, FavoriteKey, FavoriteKind, Favorites, Genre, LyricLine, Lyrics,
    Playlist, SearchResults, Song,
};
pub use playback::{
    PlayModes, PlaybackId, PlaybackMode, RepeatMode, TranscodingQuality, VolumeNormalization,
};
pub use preferences::{LanguagePreference, ThemePreference};
pub use queue::{PlaybackQueue, PlaybackSession, QueueEntry, QueueEntryId, QueueOrigin};
pub use server::{CapabilitySupport, OpenSubsonicExtension, ServerCapabilities, ServerInfo};
pub use shortcuts::{GlobalShortcutSettings, ShortcutAction, ShortcutSettings};
pub use sort::{AlbumSort, ArtistSort};

#[cfg(test)]
mod tests;
