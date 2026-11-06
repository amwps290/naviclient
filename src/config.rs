//! Persistence facade for global preferences, server data, and cover assets.

mod covers;
mod json;
mod migration;
mod paths;
mod preferences;
mod server_cache;
mod server_writer;

pub(crate) use server_cache::CacheSnapshot;
pub(crate) use server_writer::ServerCacheWriter;

pub use covers::{
    cover_cache_file_name, cover_cache_path, prune_cover_cache, read_cover_cache, write_cover_cache,
};
pub use paths::{
    audio_cache_dir, cover_cache_dir, default_audio_cache_dir, log_path, server_scope,
};
pub use preferences::{load, save};
pub use server_cache::{
    load_daily_mix, load_genres_cache, load_listening_history, load_loudness, load_session,
    remove_server_cache,
};
