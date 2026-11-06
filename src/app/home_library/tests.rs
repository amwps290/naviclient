use super::*;

fn album(id: &str) -> Album {
    Album {
        id: id.into(),
        cover_art: Some(format!("cover-{id}")),
        ..Default::default()
    }
}

#[test]
fn all_sections_complete_independently_in_different_orders() {
    for rotation in 0..4 {
        let mut home = HomeAlbums::default();
        let mut requests = LibraryRequests::default();
        let pending =
            HomeAlbumSection::ALL.map(|section| (section, home.begin(section, &mut requests)));
        for index in 0..4 {
            let (section, request) = pending[(index + rotation) % 4];
            let other = pending[(index + rotation + 1) % 4].1;
            assert!(home
                .complete(section, other, Ok(vec![album("wrong request")]))
                .is_none());
            home.complete(section, request, Ok(vec![album(section.key())]));
            assert!(!home.loading(section));
            assert_eq!(home.albums(section)[0].id, section.key());
            for &(next_section, _) in &pending {
                if home.albums(next_section).is_empty() {
                    assert!(home.loading(next_section));
                }
            }
        }
    }
}

#[test]
fn refreshing_a_section_ignores_old_results_and_duplicate_errors() {
    let mut home = HomeAlbums::default();
    let mut requests = LibraryRequests::default();
    let section = HomeAlbumSection::Random;
    let old = home.begin(section, &mut requests);
    let current = home.begin(section, &mut requests);
    assert!(home
        .complete(section, old, Ok(vec![album("stale")]))
        .is_none());
    assert!(home
        .complete(section, old, Err("stale error".into()))
        .is_none());
    assert!(home.loading(section));
    home.complete(section, current, Ok(vec![album("new")]));
    assert!(home
        .complete(section, current, Err("duplicate error".into()))
        .is_none());
    assert_eq!(home.albums(section)[0].id, "new");
    assert!(!home.loading(section));
}

#[test]
fn failures_keep_existing_data_and_successful_empty_results_clear_it() {
    let mut home = HomeAlbums::default();
    let mut requests = LibraryRequests::default();
    for section in HomeAlbumSection::ALL {
        let first = home.begin(section, &mut requests);
        home.complete(section, first, Ok(vec![album("cached")]));
        let refresh = home.begin(section, &mut requests);
        assert_eq!(
            home.complete(section, refresh, Err("offline".into()))
                .unwrap()
                .unwrap_err(),
            "offline"
        );
        assert_eq!(home.albums(section)[0].id, "cached");
        assert!(!home.loading(section));
        let retry = home.begin(section, &mut requests);
        home.complete(section, retry, Ok(vec![]));
        assert!(home.albums(section).is_empty());
    }
}

#[test]
fn server_reset_rejects_all_old_section_responses() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let old = HomeAlbumSection::ALL
        .map(|section| (section, state.home_albums.begin(section, &mut requests)));
    state = AppState::default();
    let current = HomeAlbumSection::ALL
        .map(|section| (section, state.home_albums.begin(section, &mut requests)));
    for (section, request) in old {
        assert!(state
            .home_albums
            .complete(section, request, Ok(vec![album("old server")]))
            .is_none());
        assert!(state
            .home_albums
            .complete(section, request, Err("old server error".into()))
            .is_none());
        assert!(state.home_albums.loading(section));
    }
    for (section, request) in current {
        state
            .home_albums
            .complete(section, request, Ok(vec![album("new server")]));
        assert_eq!(state.home_albums.albums(section)[0].id, "new server");
    }
}

#[test]
fn recent_artwork_is_prefetched_separately_and_previews_are_capped() {
    let mut albums: Vec<_> = (0..40).map(|id| album(&id.to_string())).collect();
    for section in HomeAlbumSection::ALL {
        let (featured, covers) = home_album_covers(section, &albums);
        assert_eq!(covers.len(), 30);
        if section == HomeAlbumSection::Recent {
            assert_eq!(featured.as_deref(), Some("cover-0"));
            assert_eq!(covers.first().map(String::as_str), Some("cover-1"));
        } else {
            assert!(featured.is_none());
            assert_eq!(covers.first().map(String::as_str), Some("cover-0"));
        }
    }
    albums[0].cover_art = None;
    let (featured, covers) = home_album_covers(HomeAlbumSection::Recent, &albums);
    assert!(featured.is_none());
    assert_eq!(covers.first().map(String::as_str), Some("cover-1"));
    assert_eq!(covers.len(), 30);
}

#[test]
fn accepted_sections_keep_server_order_and_expose_borrowed_data() {
    let mut home = HomeAlbums::default();
    let mut requests = LibraryRequests::default();
    let section = HomeAlbumSection::Newest;
    let request = home.begin(section, &mut requests);
    let mut second = album("second");
    second.artist_id = Some("artist".into());
    second.year = Some(2026);
    let albums = home
        .complete(section, request, Ok(vec![second, album("first")]))
        .unwrap()
        .unwrap();
    assert_eq!(
        albums
            .iter()
            .map(|album| album.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
    assert_eq!(albums[0].artist_id.as_deref(), Some("artist"));
    assert_eq!(albums[0].year, Some(2026));
    let data = albums.as_ptr();
    assert_eq!(data, home.albums(section).as_ptr());
}
