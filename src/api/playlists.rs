//! Playlist reads and ownership-checked writes with repeated form parameters.

use anyhow::{anyhow, Context, Result};
use rust_i18n::t;

use super::OpenSubsonicProvider;
use crate::models::Playlist;
use crate::playlist::{can_edit, song_ids, PlaylistDetails, PlaylistEdit, PlaylistEdited};

impl OpenSubsonicProvider {
    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        let body = self.get_json("getPlaylists", &[]).await?;
        let mut playlists = Vec::new();
        for playlist in body["playlists"]["playlist"]
            .as_array()
            .into_iter()
            .flatten()
        {
            playlists.push(serde_json::from_value(playlist.clone())?);
        }
        Ok(playlists)
    }

    pub async fn playlist(&self, playlist_id: &str) -> Result<PlaylistDetails> {
        let body = self.get_json("getPlaylist", &[("id", playlist_id)]).await?;
        Ok(serde_json::from_value(body["playlist"].clone())?)
    }

    pub async fn edit_playlist(&self, edit: &PlaylistEdit) -> Result<PlaylistEdited> {
        if let Some(id) = edit.id() {
            let current = self.playlist(id).await?;
            if !can_edit(&current.playlist, &self.username) {
                return Err(anyhow!(t!(
                    "Only the playlist owner can edit this playlist."
                )));
            }
            edit.validate_entries(&song_ids(&current.songs))?;
        }
        let mut params: Vec<(&str, String)> = Vec::new();
        let endpoint = match edit {
            PlaylistEdit::Create { name, songs } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(anyhow!(t!("Enter a playlist name.")));
                }
                params.push(("name", name.into()));
                params.extend(songs.iter().cloned().map(|id| ("songId", id)));
                "createPlaylist"
            }
            PlaylistEdit::Rename { id, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(anyhow!(t!("Enter a playlist name.")));
                }
                params.extend([("playlistId", id.clone()), ("name", name.into())]);
                "updatePlaylist"
            }
            PlaylistEdit::Delete { id } => {
                params.push(("id", id.clone()));
                "deletePlaylist"
            }
            PlaylistEdit::Append { id, songs } => {
                if songs.is_empty() {
                    return Err(anyhow!(t!("Choose at least one song.")));
                }
                params.push(("playlistId", id.clone()));
                params.extend(songs.iter().cloned().map(|id| ("songIdToAdd", id)));
                "updatePlaylist"
            }
            PlaylistEdit::Remove { id, index, .. } => {
                params.extend([
                    ("playlistId", id.clone()),
                    ("songIndexToRemove", index.to_string()),
                ]);
                "updatePlaylist"
            }
            PlaylistEdit::Move {
                id,
                expected,
                from,
                to,
            } => {
                let mut songs = expected.clone();
                let song = songs.remove(*from);
                songs.insert(*to, song);
                params.push(("playlistId", id.clone()));
                params.extend(songs.into_iter().map(|id| ("songId", id)));
                "createPlaylist"
            }
        };
        let params: Vec<_> = params
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let body = self.request_json(endpoint, &params, true).await?;
        let id = match edit.id() {
            Some(id) => id.to_string(),
            None => body["playlist"]["id"]
                .as_str()
                .context("Created playlist response is missing its ID")?
                .to_string(),
        };
        // Read-after-write refreshes are separate: a failed refresh must never
        // present a successful append as failed and encourage adding it twice.
        Ok(PlaylistEdited {
            id,
            deleted: matches!(edit, PlaylistEdit::Delete { .. }),
        })
    }
}

#[cfg(test)]
mod tests;
