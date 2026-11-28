//! Persistent local favorites store for a LocalFolder source.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::models::{FavoriteKey, FavoriteKind};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct LocalFavoritesStore {
    pub(super) songs: BTreeSet<String>,
    pub(super) albums: BTreeSet<String>,
    pub(super) artists: BTreeSet<String>,
}

impl LocalFavoritesStore {
    pub(super) fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        crate::config::write_json_atomic(path, self, "local favorites")
    }

    pub(super) fn set(&mut self, key: &FavoriteKey, starred: bool) {
        let target = match key.kind {
            FavoriteKind::Song => &mut self.songs,
            FavoriteKind::Album => &mut self.albums,
            FavoriteKind::Artist => &mut self.artists,
        };
        if starred {
            target.insert(key.id.clone());
        } else {
            target.remove(&key.id);
        }
    }
}
