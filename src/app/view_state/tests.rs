use super::*;

const VIEWS: [View; 12] = [
    View::Home,
    View::Favorites,
    View::Artists,
    View::Albums,
    View::Genres,
    View::Playlists,
    View::Search,
    View::NowPlaying,
    View::ArtistDetail,
    View::AlbumDetail,
    View::GenreDetail,
    View::PlaylistDetail,
];

#[test]
fn detail_views_return_to_their_own_library_section() {
    for (detail, parent) in [
        (View::ArtistDetail, View::Artists),
        (View::AlbumDetail, View::Albums),
        (View::GenreDetail, View::Genres),
        (View::PlaylistDetail, View::Playlists),
    ] {
        assert_eq!(detail.detail_parent(), Some(parent));
        assert_eq!(detail.nav_id(), parent.nav_id());
    }
}

#[test]
fn top_level_views_have_no_detail_back_target() {
    for view in [
        View::Home,
        View::Favorites,
        View::Artists,
        View::Albums,
        View::Genres,
        View::Playlists,
        View::Search,
        View::NowPlaying,
    ] {
        assert_eq!(view.detail_parent(), None, "{view:?}");
    }
}

#[test]
fn sidebar_selection_keeps_detail_pages_in_their_own_section() {
    for current in VIEWS {
        let selected: &[View] = match current {
            View::ArtistDetail => &[View::ArtistDetail, View::Artists],
            View::AlbumDetail => &[View::AlbumDetail, View::Albums],
            View::GenreDetail => &[View::GenreDetail, View::Genres],
            View::PlaylistDetail => &[View::PlaylistDetail, View::Playlists],
            _ => std::slice::from_ref(&current),
        };
        for candidate in VIEWS {
            assert_eq!(
                candidate.is_selected_for(current),
                selected.contains(&candidate),
                "candidate={candidate:?}, current={current:?}",
            );
        }
    }
}

#[test]
fn navigation_ids_keep_each_section_distinct() {
    for (view, id) in [
        (View::Home, "home"),
        (View::Favorites, "favorites"),
        (View::Artists, "artists"),
        (View::Albums, "albums"),
        (View::Genres, "genres"),
        (View::Playlists, "playlists"),
        (View::Search, "search"),
        (View::NowPlaying, "now-playing"),
    ] {
        assert_eq!(view.nav_id(), id);
    }
}
