use super::*;
use crate::app::shell::view_helpers::item_instance_key;
use crate::app::songs::selection::SongSelection;
use std::collections::HashSet;

#[test]
fn typed_scopes_keep_all_existing_element_id_spellings_distinct() {
    let expected = [
        (SongScope::AlbumDetail, "album-detail"),
        (SongScope::ArtistTop, "artist-top"),
        (SongScope::PlaylistDetail, "playlist-detail"),
        (SongScope::Favorites, "favorites"),
        (SongScope::Search, "search"),
        (SongScope::Queue, "queue"),
        (SongScope::History, "history"),
    ];
    assert_eq!(SongScope::ALL, expected.map(|(scope, _)| scope));
    assert_eq!(SongScope::ALL.into_iter().collect::<HashSet<_>>().len(), 7);
    assert_eq!(
        expected
            .map(|(_, id)| id)
            .into_iter()
            .collect::<HashSet<_>>()
            .len(),
        7
    );
    for (scope, id) in expected {
        assert_eq!(scope.id(), id);
        assert_eq!(scope.to_string(), id);
        assert_eq!(format!("song-list-{scope}"), format!("song-list-{id}"));
        for action in ["play", "next", "queue", "playlist", "favorite", "clear"] {
            assert_eq!(
                format!("multi-{action}-{scope}"),
                format!("multi-{action}-{id}")
            );
        }
        assert_eq!(
            item_instance_key(scope.id(), "song", 1, "duplicate"),
            format!("{id}/song/1/duplicate")
        );
    }
}

#[test]
fn active_scopes_match_each_page_and_an_open_drawer_always_takes_priority() {
    let pages = [
        (View::Home, None),
        (View::Favorites, Some(SongScope::Favorites)),
        (View::Artists, None),
        (View::Albums, None),
        (View::Genres, None),
        (View::GenreDetail, None),
        (View::Playlists, None),
        (View::Search, Some(SongScope::Search)),
        (View::ArtistDetail, Some(SongScope::ArtistTop)),
        (View::AlbumDetail, Some(SongScope::AlbumDetail)),
        (View::PlaylistDetail, Some(SongScope::PlaylistDetail)),
        (View::NowPlaying, None),
    ];
    for (view, expected) in pages {
        for tab in QueueDrawerTab::ALL {
            assert_eq!(SongScope::active(view, false, tab), expected);
            assert_eq!(
                SongScope::active(view, true, tab),
                Some(match tab {
                    QueueDrawerTab::UpNext => SongScope::Queue,
                    QueueDrawerTab::History => SongScope::History,
                })
            );
        }
    }
}

#[test]
fn list_layout_and_replacement_origins_keep_their_existing_scope_rules() {
    for (scope, fills_viewport, drawer, origin) in [
        (SongScope::AlbumDetail, true, false, QueueOrigin::Manual),
        (SongScope::ArtistTop, false, false, QueueOrigin::Manual),
        (
            SongScope::PlaylistDetail,
            true,
            false,
            QueueOrigin::Playlist,
        ),
        (SongScope::Favorites, false, false, QueueOrigin::Manual),
        (SongScope::Search, false, false, QueueOrigin::Manual),
        (SongScope::Queue, true, true, QueueOrigin::Manual),
        (SongScope::History, true, true, QueueOrigin::Manual),
    ] {
        assert_eq!(scope.fills_viewport(), fills_viewport);
        assert_eq!(scope.is_drawer(), drawer);
        assert_eq!(scope.queue_origin(), origin);
    }
}

#[test]
fn selection_clear_and_remap_cannot_change_any_other_typed_scope() {
    for scope in SongScope::ALL {
        let mut selection = SongSelection::default();
        selection.select(scope, 1, 4);
        selection.range(scope, 3, 4);
        for other in SongScope::ALL {
            let mut changed = selection.clone();
            if other == scope {
                changed.clear_scope(other);
                assert_eq!(changed, SongSelection::default());
                continue;
            }
            assert!(!changed.contains(other, 1));
            assert_eq!(changed.selected_indices(other, 4).count(), 0);
            changed.clear_scope(other);
            assert_eq!(changed, selection);
            changed.remap_scope(other, 4, 0, |_| None);
            assert_eq!(changed, selection);
        }
    }
}

#[test]
fn switching_between_every_scope_pair_starts_a_new_selection_and_anchor() {
    for from in SongScope::ALL {
        for to in SongScope::ALL {
            if from == to {
                continue;
            }
            let mut selection = SongSelection::default();
            selection.select(from, 3, 4);
            selection.range(from, 1, 4);
            selection.range(to, 0, 2);
            assert_eq!(selection.cursor(), Some((to, 0)));
            assert_eq!(selection.multi().unwrap().scope, to);
            assert_eq!(selection.multi().unwrap().anchor, 0);
            assert_eq!(selection.selected_indices(to, 2).collect::<Vec<_>>(), [0]);
            assert_eq!(selection.selected_indices(from, 4).count(), 0);
        }
    }
}
