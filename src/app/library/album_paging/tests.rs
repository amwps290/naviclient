use super::*;

fn album(id: &str, created: Option<&str>) -> Album {
    Album {
        id: id.to_string(),
        cover_art: Some(format!("cover-{id}")),
        created: created.map(str::to_string),
        ..Album::default()
    }
}

#[test]
fn newest_pages_correct_creation_order_and_prefetch_only_the_appended_page() {
    let mut collection = PagedCollection::default();
    let mut requests = PageRequests::default();
    let first = collection.begin(&mut requests).unwrap();
    let first_page = vec![
        album("old", Some("2025-01-01T00:00:00Z")),
        album("unknown", None),
        album("new", Some("2026-01-01T00:00:00Z")),
    ];
    assert_eq!(
        complete_album_page(
            &mut collection,
            first,
            Ok(first_page),
            Some(AlbumSort::NewestAdded)
        ),
        Some(Ok(vec![
            "cover-new".into(),
            "cover-old".into(),
            "cover-unknown".into()
        ]))
    );
    let second = collection.begin(&mut requests).unwrap();
    assert_eq!(
        complete_album_page(
            &mut collection,
            second,
            Ok(vec![album("older", Some("2024-01-01T00:00:00Z"))]),
            Some(AlbumSort::NewestAdded),
        ),
        Some(Ok(vec!["cover-older".into()]))
    );
    let ids: Vec<_> = collection
        .items()
        .iter()
        .map(|album| album.id.as_str())
        .collect();
    assert_eq!(ids, ["new", "old", "unknown", "older"]);
}

#[test]
fn other_sorts_and_genre_pages_preserve_server_order() {
    for sort in [
        Some(AlbumSort::Name),
        Some(AlbumSort::Artist),
        Some(AlbumSort::Year),
        None,
    ] {
        let mut collection = PagedCollection::default();
        let mut requests = PageRequests::default();
        let request = collection.begin(&mut requests).unwrap();
        let page = vec![album("b", None), album("a", Some("2026-01-01"))];
        assert_eq!(
            complete_album_page(&mut collection, request, Ok(page), sort),
            Some(Ok(vec!["cover-b".into(), "cover-a".into()]))
        );
        let ids: Vec<_> = collection
            .items()
            .iter()
            .map(|album| album.id.as_str())
            .collect();
        assert_eq!(ids, ["b", "a"]);
    }
}

#[test]
fn artwork_prefetch_is_capped_without_dropping_albums() {
    let mut collection = PagedCollection::default();
    let mut requests = PageRequests::default();
    let request = collection.begin(&mut requests).unwrap();
    let page = (0..100).map(|id| album(&id.to_string(), None)).collect();
    let covers = complete_album_page(&mut collection, request, Ok(page), None)
        .unwrap()
        .unwrap();
    assert_eq!(covers.len(), 80);
    assert_eq!(collection.len(), 100);
}

#[test]
fn refreshing_or_changing_sort_ignores_old_pages_and_errors() {
    let mut state = AppState {
        albums_sort: AlbumSort::Name,
        ..AppState::default()
    };
    let mut requests = PageRequests::default();
    let old = state.albums.begin(&mut requests).unwrap();
    state.albums_sort = AlbumSort::NewestAdded;
    state.albums.reset();
    let current = state.albums.begin(&mut requests).unwrap();
    assert_eq!(
        complete_album_page(
            &mut state.albums,
            old,
            Ok(vec![album("stale", None)]),
            Some(state.albums_sort)
        ),
        None
    );
    assert_eq!(
        complete_album_page(
            &mut state.albums,
            old,
            Err("stale error".into()),
            Some(state.albums_sort)
        ),
        None
    );
    assert!(state.albums.accepts(current));
    assert!(state.albums.is_empty());
    assert_eq!(
        complete_album_page(
            &mut state.albums,
            current,
            Ok(vec![album("current", None)]),
            Some(state.albums_sort)
        ),
        Some(Ok(vec!["cover-current".into()]))
    );
}

#[test]
fn resetting_server_state_cannot_reuse_outstanding_request_ids() {
    let mut state = AppState::default();
    let mut requests = PageRequests::default();
    let old_album = state.albums.begin(&mut requests).unwrap();
    state.genres.select("Rock".into());
    let (_, old_genre) = state.genres.begin_albums(&mut requests).unwrap();
    state = AppState::default();
    let current_album = state.albums.begin(&mut requests).unwrap();
    state.genres.select("Rock".into());
    let (_, current_genre) = state.genres.begin_albums(&mut requests).unwrap();
    assert_eq!(old_album.page(), current_album.page());
    assert_eq!(old_genre.page(), current_genre.page());
    for old in [old_album, old_genre] {
        assert_eq!(
            complete_album_page(
                &mut state.albums,
                old,
                Ok(vec![album("old-server", None)]),
                None
            ),
            None
        );
        assert_eq!(
            state
                .genres
                .complete_albums(old, Err("old-server error".into())),
            None
        );
    }
    assert!(state.albums.accepts(current_album));
    assert!(state
        .genres
        .detail()
        .unwrap()
        .albums()
        .accepts(current_genre));
}
