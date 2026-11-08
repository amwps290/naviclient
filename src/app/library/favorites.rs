//! Confirmed favorite snapshots and optimistic, request-scoped changes.

use super::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::app::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FavoriteChange {
    request: LibraryRequest,
    key: FavoriteKey,
    starred: bool,
}

#[derive(Default)]
pub(in crate::app) struct FavoritesState {
    items: Favorites,
    confirmed_ids: HashSet<FavoriteKey>,
    pending: HashMap<FavoriteKey, FavoriteChange>,
    load: LibraryLoad,
}

impl FavoritesState {
    pub(in crate::app) fn items(&self) -> &Favorites {
        &self.items
    }

    pub(in crate::app) fn total(&self) -> usize {
        self.items.artists.len() + self.items.albums.len() + self.items.songs.len()
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn is_starred(&self, key: &FavoriteKey) -> bool {
        self.pending
            .get(key)
            .map_or_else(|| self.confirmed_ids.contains(key), |change| change.starred)
    }

    pub(in crate::app) fn is_pending(&self, key: &FavoriteKey) -> bool {
        self.pending.contains_key(key)
    }

    pub(in crate::app) fn begin_load(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        self.load.begin(requests)
    }

    pub(in crate::app) fn complete_load(
        &mut self,
        request: LibraryRequest,
        result: Result<Favorites, String>,
    ) -> Option<Result<&Favorites, &str>> {
        Some(match self.load.complete(request, result)? {
            Ok(items) => {
                let mut ids = HashSet::new();
                ids.extend(
                    items
                        .artists
                        .iter()
                        .map(|artist| FavoriteKey::new(FavoriteKind::Artist, &artist.id)),
                );
                ids.extend(
                    items
                        .albums
                        .iter()
                        .map(|album| FavoriteKey::new(FavoriteKind::Album, &album.id)),
                );
                ids.extend(
                    items
                        .songs
                        .iter()
                        .map(|song| FavoriteKey::new(FavoriteKind::Song, &song.id)),
                );
                self.items = items;
                self.confirmed_ids = ids;
                Ok(&self.items)
            }
            Err(error) => Err(error),
        })
    }

    pub(in crate::app) fn begin_change(
        &mut self,
        key: FavoriteKey,
        requests: &mut LibraryRequests,
    ) -> Option<FavoriteChange> {
        if self.is_pending(&key) {
            return None;
        }
        let change = FavoriteChange {
            request: requests.next(),
            starred: !self.is_starred(&key),
            key,
        };
        self.pending.insert(change.key.clone(), change.clone());
        Some(change)
    }

    fn complete_change(
        &mut self,
        change: &FavoriteChange,
        result: Result<(), String>,
    ) -> Option<Result<(), String>> {
        if self.pending.get(&change.key) != Some(change) {
            return None;
        }
        self.pending.remove(&change.key);
        if result.is_ok() {
            if change.starred {
                self.confirmed_ids.insert(change.key.clone());
            } else {
                self.confirmed_ids.remove(&change.key);
            }
            // Reads started before acknowledgment can contain the old value.
            // The caller starts a fresh read after each successful change.
            self.load = LibraryLoad::default();
        }
        // Failed changes expose the latest confirmed snapshot; they never
        // blindly invert an optimistic value copied into the server index.
        Some(result)
    }
}

impl NavidromeApp {
    pub(in crate::app) fn load_favorites(&mut self) {
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.capabilities().favorite_read)
        else {
            return;
        };
        let request = self.state.favorites.begin_load(&mut self.library_requests);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.favorites().await.map_err(error_message);
            let _ = tx.send(Msg::Favorites { request, result });
        });
    }

    pub(in crate::app) fn toggle_favorite(&mut self, key: FavoriteKey) {
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.capabilities().favorite_write)
        else {
            return;
        };
        let Some(change) = self
            .state
            .favorites
            .begin_change(key, &mut self.library_requests)
        else {
            return;
        };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .set_favorite(&change.key, change.starred)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::FavoriteChanged { change, result });
        });
    }

    pub(in crate::app) fn finish_favorites(
        &mut self,
        request: LibraryRequest,
        result: Result<Favorites, String>,
    ) {
        let Some(result) = self.state.favorites.complete_load(request, result) else {
            return;
        };
        match result {
            Ok(favorites) => {
                let covers = favorites
                    .albums
                    .iter()
                    .filter_map(|album| album.cover_art.clone())
                    .chain(
                        favorites
                            .artists
                            .iter()
                            .filter_map(|artist| artist.cover_art.clone()),
                    )
                    .chain(
                        favorites
                            .songs
                            .iter()
                            .filter_map(|song| song.cover_art.clone()),
                    )
                    .collect();
                self.state
                    .clear_song_selection_for_scope(SongScope::Favorites);
                self.preload_covers(covers);
            }
            Err(error) => {
                let error = error.to_string();
                self.notify_error(error);
            }
        }
    }

    pub(in crate::app) fn finish_favorite_change(
        &mut self,
        change: FavoriteChange,
        result: Result<(), String>,
    ) {
        let Some(result) = self.state.favorites.complete_change(&change, result) else {
            return;
        };
        match result {
            Ok(()) => self.load_favorites(),
            Err(error) => {
                let action = if change.starred {
                    t!("add")
                } else {
                    t!("remove")
                };
                self.notify_error(t!(
                    "Failed to %{action} favorite: %{error}",
                    action = action,
                    error = error
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests;
