//! Playlist edits preserve occurrences: song IDs identify media, indexes identify
//! entries. Never deduplicate an ordered playlist or a saved playback queue.
use anyhow::{bail, Result};
use serde::Deserialize;

use crate::models::{Playlist, Song};

#[derive(Clone, Debug, Deserialize)]
pub struct PlaylistDetails {
    #[serde(flatten)]
    pub playlist: Playlist,
    #[serde(default, rename = "entry")]
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug)]
pub enum PlaylistEdit {
    Create {
        name: String,
        songs: Vec<String>,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
    Append {
        id: String,
        songs: Vec<String>,
    },
    Remove {
        id: String,
        expected: Vec<String>,
        index: usize,
    },
    Move {
        id: String,
        expected: Vec<String>,
        from: usize,
        to: usize,
    },
}

impl PlaylistEdit {
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Create { .. } => None,
            Self::Rename { id, .. }
            | Self::Delete { id }
            | Self::Append { id, .. }
            | Self::Remove { id, .. }
            | Self::Move { id, .. } => Some(id),
        }
    }

    pub fn validate_entries(&self, current: &[String]) -> Result<()> {
        let (expected, valid) = match self {
            Self::Remove {
                expected, index, ..
            } => (expected, *index < current.len()),
            Self::Move {
                expected, from, to, ..
            } => (expected, *from < current.len() && *to < current.len()),
            _ => return Ok(()),
        };
        if expected != current || !valid {
            bail!("This playlist changed on the server. Refresh it and try again.");
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct PlaylistEdited {
    pub id: String,
    pub deleted: bool,
}

pub fn song_ids(songs: &[Song]) -> Vec<String> {
    songs.iter().map(|song| song.id.clone()).collect()
}

pub fn can_edit(playlist: &Playlist, username: &str) -> bool {
    playlist
        .owner
        .as_deref()
        .is_none_or(|owner| owner == username)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrence_edits_reject_stale_order_and_out_of_bounds_indexes() {
        let expected = vec!["a".into(), "b".into(), "a".into()];
        let edit = PlaylistEdit::Remove {
            id: "p".into(),
            expected: expected.clone(),
            index: 2,
        };
        assert!(edit.validate_entries(&expected).is_ok());
        assert!(edit
            .validate_entries(&["a".into(), "a".into(), "b".into()])
            .is_err());
        assert!(edit.validate_entries(&expected[..2]).is_err());
        let edit = PlaylistEdit::Move {
            id: "p".into(),
            expected: expected.clone(),
            from: 0,
            to: 3,
        };
        assert!(edit.validate_entries(&expected).is_err());
    }

    #[test]
    fn public_playlists_owned_by_someone_else_are_read_only() {
        let playlist = Playlist {
            owner: Some("alice".into()),
            public: Some(true),
            ..Default::default()
        };
        assert!(can_edit(&playlist, "alice"));
        assert!(!can_edit(&playlist, "bob"));
    }
}
