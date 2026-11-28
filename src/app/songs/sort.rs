//! Non-destructive multi-column song list sorting with `original_index` preservation.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::scope::SongScope;
use crate::models::Song;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::app) enum SongSortColumn {
    Track,
    Title,
    Artist,
    Duration,
}

impl SongSortColumn {
    pub(in crate::app) const fn id(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Duration => "duration",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::app) enum SortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::app) struct SongSort {
    pub(in crate::app) column: SongSortColumn,
    pub(in crate::app) direction: SortDirection,
}

impl SongSort {
    pub(in crate::app) fn next(current: Option<Self>, column: SongSortColumn) -> Option<Self> {
        match current {
            Some(active) if active.column == column => match active.direction {
                SortDirection::Asc => Some(Self {
                    column,
                    direction: SortDirection::Desc,
                }),
                SortDirection::Desc => None,
            },
            _ => Some(Self {
                column,
                direction: SortDirection::Asc,
            }),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(in crate::app) struct SongSortState {
    sorts: HashMap<SongScope, SongSort>,
    indices: HashMap<SongScope, Vec<usize>>,
}

impl SongSortState {
    pub(in crate::app) fn active(&self, scope: SongScope) -> Option<SongSort> {
        self.sorts.get(&scope).copied()
    }

    pub(in crate::app) fn is_sorted(&self, scope: SongScope) -> bool {
        self.sorts.contains_key(&scope)
    }

    pub(in crate::app) fn clear_scope(&mut self, scope: SongScope) {
        self.sorts.remove(&scope);
        self.indices.remove(&scope);
    }

    pub(in crate::app) fn clear_all(&mut self) {
        self.sorts.clear();
        self.indices.clear();
    }

    /// Cycle sort column on a browsing scope (`None -> Asc -> Desc -> None`) and recompute indices.
    pub(in crate::app) fn toggle(
        &mut self,
        scope: SongScope,
        column: SongSortColumn,
        songs: &[Song],
        subset: Option<&[usize]>,
    ) -> Option<SongSort> {
        if !scope.is_sortable() {
            return None;
        }
        let next = SongSort::next(self.active(scope), column);
        match next {
            Some(sort) => {
                self.sorts.insert(scope, sort);
                let order = compute_sorted_indices(songs, subset, sort);
                self.indices.insert(scope, order);
            }
            None => {
                self.sorts.remove(&scope);
                self.indices.remove(&scope);
            }
        }
        next
    }

    /// Recompute cached indices if a sort is currently active for `scope`.
    pub(in crate::app) fn refresh(
        &mut self,
        scope: SongScope,
        songs: &[Song],
        subset: Option<&[usize]>,
    ) {
        if let Some(sort) = self.active(scope) {
            self.indices
                .insert(scope, compute_sorted_indices(songs, subset, sort));
        } else {
            self.indices.remove(&scope);
        }
    }

    /// Borrow sorted `original_index` mapping if active and length-consistent with the backing list.
    pub(in crate::app) fn indices(&self, scope: SongScope, expected_len: usize) -> Option<&[usize]> {
        self.sorts.get(&scope)?;
        let cached = self.indices.get(&scope)?;
        (cached.len() == expected_len).then_some(cached.as_slice())
    }
}

pub(in crate::app) fn compute_sorted_indices(
    songs: &[Song],
    subset: Option<&[usize]>,
    sort: SongSort,
) -> Vec<usize> {
    let mut indices: Vec<usize> = match subset {
        Some(slice) => slice.iter().copied().filter(|&i| i < songs.len()).collect(),
        None => (0..songs.len()).collect(),
    };
    indices.sort_by(|&a, &b| {
        let ord = compare_songs(&songs[a], &songs[b], sort.column);
        let directed = match sort.direction {
            SortDirection::Asc => ord,
            SortDirection::Desc => ord.reverse(),
        };
        directed.then_with(|| a.cmp(&b))
    });
    indices
}

fn compare_songs(a: &Song, b: &Song, column: SongSortColumn) -> Ordering {
    match column {
        SongSortColumn::Track => {
            let disc_a = a.disc.unwrap_or(0);
            let disc_b = b.disc.unwrap_or(0);
            let track_a = a.track.unwrap_or(i32::MAX);
            let track_b = b.track.unwrap_or(i32::MAX);
            disc_a
                .cmp(&disc_b)
                .then_with(|| track_a.cmp(&track_b))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }
        SongSortColumn::Title => a
            .title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.artist.to_lowercase().cmp(&b.artist.to_lowercase())),
        SongSortColumn::Artist => a
            .artist
            .to_lowercase()
            .cmp(&b.artist.to_lowercase())
            .then_with(|| a.album.to_lowercase().cmp(&b.album.to_lowercase()))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        SongSortColumn::Duration => {
            let dur_a = a.duration.filter(|&d| d > 0).unwrap_or(i32::MAX);
            let dur_b = b.duration.filter(|&d| d > 0).unwrap_or(i32::MAX);
            dur_a
                .cmp(&dur_b)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, artist: &str, track: Option<i32>, duration: Option<i32>) -> Song {
        Song {
            id: title.to_lowercase(),
            title: title.into(),
            artist: artist.into(),
            track,
            duration,
            ..Default::default()
        }
    }

    #[test]
    fn sort_cycles_default_asc_desc_default_and_blocks_queue_and_history() {
        let songs = vec![
            song("Zebra", "Bob", Some(3), Some(240)),
            song("Alpha", "Charlie", Some(1), Some(120)),
            song("Middle", "Alice", Some(2), Some(180)),
        ];
        let mut state = SongSortState::default();

        // Queue and History must never sort.
        assert!(state
            .toggle(SongScope::Queue, SongSortColumn::Title, &songs, None)
            .is_none());
        assert!(state
            .toggle(SongScope::History, SongSortColumn::Title, &songs, None)
            .is_none());

        // Title Asc -> Alpha(1), Middle(2), Zebra(0)
        let asc = state
            .toggle(SongScope::Favorites, SongSortColumn::Title, &songs, None)
            .unwrap();
        assert_eq!(asc.direction, SortDirection::Asc);
        assert_eq!(state.indices(SongScope::Favorites, 3), Some(&[1, 2, 0][..]));

        // Title Desc -> Zebra(0), Middle(2), Alpha(1)
        let desc = state
            .toggle(SongScope::Favorites, SongSortColumn::Title, &songs, None)
            .unwrap();
        assert_eq!(desc.direction, SortDirection::Desc);
        assert_eq!(state.indices(SongScope::Favorites, 3), Some(&[0, 2, 1][..]));

        // Third click resets to None (default order)
        assert!(state
            .toggle(SongScope::Favorites, SongSortColumn::Title, &songs, None)
            .is_none());
        assert!(state.indices(SongScope::Favorites, 3).is_none());

        // Artist Asc -> Alice(2), Bob(0), Charlie(1)
        state.toggle(SongScope::Favorites, SongSortColumn::Artist, &songs, None);
        assert_eq!(state.indices(SongScope::Favorites, 3), Some(&[2, 0, 1][..]));

        // Duration Asc -> 120(1), 180(2), 240(0)
        state.toggle(SongScope::Favorites, SongSortColumn::Duration, &songs, None);
        assert_eq!(state.indices(SongScope::Favorites, 3), Some(&[1, 2, 0][..]));

        // Track Asc -> 1(1), 2(2), 3(0)
        state.toggle(SongScope::Favorites, SongSortColumn::Track, &songs, None);
        assert_eq!(state.indices(SongScope::Favorites, 3), Some(&[1, 2, 0][..]));
    }

    #[test]
    fn subset_sorting_preserves_original_backing_indices() {
        let songs = vec![
            song("Delta", "D", Some(4), Some(400)),
            song("Bravo", "B", Some(2), Some(200)),
            song("Charlie", "C", Some(3), Some(300)),
            song("Alpha", "A", Some(1), Some(100)),
        ];
        // Filtered subset contains indices [0 ("Delta"), 3 ("Alpha")]
        let subset = [0usize, 3usize];
        let mut state = SongSortState::default();
        state.toggle(
            SongScope::PlaylistDetail,
            SongSortColumn::Title,
            &songs,
            Some(&subset),
        );
        // Sorted ascending by title: Alpha (original 3), Delta (original 0)
        assert_eq!(
            state.indices(SongScope::PlaylistDetail, 2),
            Some(&[3, 0][..])
        );
    }
}
