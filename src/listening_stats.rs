//! Privacy-first local listening log (`listening-log.jsonl`) and insights aggregation.

pub mod aggregate;
pub mod log_store;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

pub use aggregate::{aggregate_insights, ListeningInsights, ListeningRange, RankedStat};
pub use log_store::ListeningLog;

pub fn default_log_path() -> PathBuf {
    crate::config::config_dir().join("listening-log.jsonl")
}
