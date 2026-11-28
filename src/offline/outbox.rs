//! Delayed sync outbox for actions performed while offline (scrobbles & favorites).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::models::{FavoriteKey, FavoriteKind};
use crate::source::SourceId;

const OUTBOX_FILE: &str = "pending-sync.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum PendingSyncOp {
    Scrobble {
        source: SourceId,
        song_id: String,
    },
    SetFavorite {
        source: SourceId,
        kind: SerializableFavoriteKind,
        id: String,
        starred: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SerializableFavoriteKind {
    Artist,
    Album,
    Song,
}

impl From<FavoriteKind> for SerializableFavoriteKind {
    fn from(kind: FavoriteKind) -> Self {
        match kind {
            FavoriteKind::Artist => Self::Artist,
            FavoriteKind::Album => Self::Album,
            FavoriteKind::Song => Self::Song,
        }
    }
}

impl From<SerializableFavoriteKind> for FavoriteKind {
    fn from(kind: SerializableFavoriteKind) -> Self {
        match kind {
            SerializableFavoriteKind::Artist => Self::Artist,
            SerializableFavoriteKind::Album => Self::Album,
            SerializableFavoriteKind::Song => Self::Song,
        }
    }
}

impl PendingSyncOp {
    pub(crate) fn favorite(source: SourceId, key: &FavoriteKey, starred: bool) -> Self {
        Self::SetFavorite {
            source,
            kind: key.kind.into(),
            id: key.id.clone(),
            starred,
        }
    }

    pub(crate) fn source(&self) -> &SourceId {
        match self {
            Self::Scrobble { source, .. } | Self::SetFavorite { source, .. } => source,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct OfflineOutbox {
    path: PathBuf,
    ops: Vec<PendingSyncOp>,
}

impl OfflineOutbox {
    pub(crate) fn open(dir: &Path) -> Self {
        let path = dir.join(OUTBOX_FILE);
        let ops = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<PendingSyncOp>>(&text).ok())
            .unwrap_or_default();
        Self { path, ops }
    }

    pub(crate) fn set_dir(&mut self, dir: &Path) {
        let next = dir.join(OUTBOX_FILE);
        if self.path == next {
            return;
        }
        *self = Self::open(dir);
    }

    pub(crate) fn push(&mut self, op: PendingSyncOp) -> Result<()> {
        // Coalesce repeated favorite toggles for the same target.
        if let PendingSyncOp::SetFavorite {
            source, kind, id, ..
        } = &op
        {
            self.ops.retain(|existing| {
                !matches!(
                    existing,
                    PendingSyncOp::SetFavorite {
                        source: es,
                        kind: ek,
                        id: eid,
                        ..
                    } if es == source && ek == kind && eid == id
                )
            });
        }
        self.ops.push(op);
        self.save()
    }

    pub(crate) fn take_for_source(&mut self, source: &SourceId) -> Vec<PendingSyncOp> {
        let mut taken = Vec::new();
        let mut remaining = Vec::new();
        for op in self.ops.drain(..) {
            if op.source() == source {
                taken.push(op);
            } else {
                remaining.push(op);
            }
        }
        self.ops = remaining;
        let _ = self.save();
        taken
    }

    pub(crate) fn len(&self) -> usize {
        self.ops.len()
    }

    fn save(&self) -> Result<()> {
        crate::config::write_json_atomic(&self.path, &self.ops, "offline sync outbox")
    }
}
