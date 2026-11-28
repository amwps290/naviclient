//! Persistence facade for global preferences, server data, and cover assets.

mod cache_files;
mod covers;
mod json;
mod migration;
mod paths;
mod preferences;
mod server_cache;
mod server_writer;
pub(crate) use cache_files::{is_audio_cache_name, prune_cache_files};
pub(crate) use json::write_json_atomic;

pub(crate) use server_cache::CacheSnapshot;
pub(crate) use server_writer::ServerCacheWriter;

pub use covers::{
    cache_usage, clear_audio_cache, clear_cover_cache, prune_cover_cache, read_source_cover_cache,
    source_cover_cache_path, write_source_cover_cache, CacheUsage,
};
pub use paths::{
    audio_cache_dir, config_dir, log_path, offline_audio_dir, prepare_audio_cache_dir,
    server_scope, source_data_dir, source_scope,
};
pub use preferences::{load, save};
pub use server_cache::{
    load_daily_mix, load_genres_cache, load_source_history, load_source_loudness,
    load_source_session, remove_source_cache,
};
