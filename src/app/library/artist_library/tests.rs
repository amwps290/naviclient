use super::*;

fn artist(name: &str, albums: Option<i32>) -> Artist {
    Artist {
        id: name.to_string(),
        name: name.to_string(),
        album_count: albums,
        cover_art: Some(format!("cover-{name}")),
    }
}

fn artists(count: usize) -> Vec<Artist> {
    (0..count)
        .rev()
        .map(|index| artist(&format!("Artist {index:04}"), Some(1)))
        .collect()
}

fn names(artists: &[Artist]) -> Vec<&str> {
    artists.iter().map(|artist| artist.name.as_str()).collect()
}

fn load(library: &mut ArtistLibrary, requests: &mut LibraryRequests, artists: Vec<Artist>) {
    let request = library.begin(requests);
    assert!(library.complete(request, Ok(artists)).unwrap().is_ok());
}

#[test]
fn large_lists_expand_in_batches_and_short_lists_never_exceed_their_length() {
    let mut requests = LibraryRequests::default();
    for count in [0, 1, 299, 300, 301, 701] {
        let mut library = ArtistLibrary::default();
        let request = library.begin(&mut requests);
        let completed = library
            .complete(request, Ok(artists(count)))
            .unwrap()
            .unwrap();
        // Artwork prefetching receives the entire sorted result; rendering borrows
        // a prefix of the same data instead of copying each batch.
        assert_eq!(completed.len(), count);
        if count > 0 {
            assert_eq!(completed[0].name, "Artist 0000");
        }
        assert_eq!(library.len(), count);
        assert_eq!(library.is_empty(), count == 0);
        assert_eq!(library.visible_items().len(), count.min(300));
        if count > 0 {
            assert!(std::ptr::eq(&library.visible_items()[0], &library.items[0]));
        }
        library.show_more();
        assert_eq!(library.visible_items().len(), count.min(600));
        for _ in 0..4 {
            library.show_more();
        }
        assert_eq!(library.visible_items().len(), count);
        assert_eq!(library.len(), count);
    }
}

#[test]
fn changing_sort_reorders_full_list_and_resets_only_when_preference_changes() {
    let mut library = ArtistLibrary::default();
    let mut requests = LibraryRequests::default();
    load(&mut library, &mut requests, artists(701));
    library.show_more();
    assert!(!library.set_sort(ArtistSort::Name));
    assert_eq!(library.visible_items().len(), 600);

    assert!(library.set_sort(ArtistSort::NameDesc));
    assert_eq!(library.sort(), ArtistSort::NameDesc);
    assert_eq!(library.visible_items().len(), 300);
    assert_eq!(library.visible_items()[0].name, "Artist 0700");
    library.show_more();
    library.show_more();
    assert_eq!(library.visible_items().last().unwrap().name, "Artist 0000");
    assert!(!library.set_sort(ArtistSort::NameDesc));
    assert_eq!(library.visible_items().len(), 701);
    assert!(library.set_sort(ArtistSort::Name));
    assert_eq!(library.visible_items().len(), 300);
    assert_eq!(library.visible_items()[0].name, "Artist 0000");
}

#[test]
fn preference_restored_before_loading_and_changes_during_loading_both_apply() {
    let mut library = ArtistLibrary::default();
    let mut requests = LibraryRequests::default();
    assert!(library.set_sort(ArtistSort::AlbumCount));
    assert!(library.visible_items().is_empty());
    load(
        &mut library,
        &mut requests,
        vec![
            artist("b", Some(3)),
            artist("a", Some(1)),
            artist("c", None),
        ],
    );
    assert_eq!(names(library.visible_items()), ["b", "a", "c"]);

    let refresh = library.begin(&mut requests);
    assert!(library.set_sort(ArtistSort::NameDesc));
    assert!(library.loading());
    assert_eq!(names(library.visible_items()), ["c", "b", "a"]);
    let result = library
        .complete(
            refresh,
            Ok(vec![
                artist("a", Some(10)),
                artist("z", Some(1)),
                artist("m", Some(20)),
            ]),
        )
        .unwrap()
        .unwrap();
    assert_eq!(names(result), ["z", "m", "a"]);
    assert!(!library.loading());
    assert_eq!(library.sort(), ArtistSort::NameDesc);
}

#[test]
fn refresh_failure_preserves_expanded_list_and_retry_resets_display_on_success() {
    let mut library = ArtistLibrary::default();
    let mut requests = LibraryRequests::default();
    load(&mut library, &mut requests, artists(650));
    library.show_more();
    let refresh = library.begin(&mut requests);
    assert!(library.loading());
    assert_eq!(library.visible_items().len(), 600);
    assert_eq!(
        library
            .complete(refresh, Err("offline".into()))
            .unwrap()
            .unwrap_err(),
        "offline"
    );
    assert!(!library.loading());
    assert_eq!(library.len(), 650);
    assert_eq!(library.visible_items().len(), 600);
    for result in [
        Ok(vec![artist("replacement", None)]),
        Err("duplicate failure".into()),
    ] {
        assert!(library.complete(refresh, result).is_none());
        assert_eq!(library.visible_items().len(), 600);
        assert_eq!(library.load.error(), Some("offline"));
    }
    let retry = library.begin(&mut requests);
    assert!(library.load.error().is_none());
    assert!(library
        .complete(refresh, Err("late failure".into()))
        .is_none());
    assert!(library.loading());
    assert_eq!(library.visible_items().len(), 600);
    assert!(library.complete(retry, Ok(artists(400))).unwrap().is_ok());
    assert_eq!(library.len(), 400);
    assert_eq!(library.visible_items().len(), 300);
}

#[test]
fn stale_and_duplicate_results_cannot_replace_data_or_collapse_expanded_display() {
    let mut library = ArtistLibrary::default();
    let mut requests = LibraryRequests::default();
    let old = library.begin(&mut requests);
    let current = library.begin(&mut requests);
    for result in [Ok(vec![artist("old", None)]), Err("old failure".into())] {
        assert!(library.complete(old, result).is_none());
        assert!(library.loading());
        assert!(library.is_empty());
    }
    assert!(library.complete(current, Ok(artists(601))).unwrap().is_ok());
    library.show_more();
    for result in [
        Ok(vec![artist("duplicate", None)]),
        Err("duplicate failure".into()),
    ] {
        assert!(library.complete(current, result).is_none());
        assert_eq!(library.len(), 601);
        assert_eq!(library.visible_items().len(), 600);
        assert_eq!(library.visible_items()[0].name, "Artist 0000");
        assert!(library.load.error().is_none());
    }
}

#[test]
fn switching_servers_drops_previous_artists_and_rejects_old_responses() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    load(&mut state.artists, &mut requests, artists(400));
    state.artists.set_sort(ArtistSort::NameDesc);
    state.artists.show_more();
    let old = state.artists.begin(&mut requests);
    state = AppState::default();
    assert!(state.artists.is_empty());
    assert!(state.artists.visible_items().is_empty());
    // The reconnect controller restores this persisted preference on new state.
    state.artists.set_sort(ArtistSort::NameDesc);
    let current = state.artists.begin(&mut requests);
    for result in [
        Ok(vec![artist("old server", None)]),
        Err("old server failure".into()),
    ] {
        assert!(state.artists.complete(old, result).is_none());
        assert!(state.artists.is_empty());
        assert!(state.artists.loading());
    }
    assert!(state
        .artists
        .complete(current, Ok(vec![artist("a", None), artist("z", None)]))
        .unwrap()
        .is_ok());
    assert_eq!(names(state.artists.visible_items()), ["z", "a"]);
    assert!(!state.artists.loading());
}

#[test]
fn empty_success_and_initial_failure_remain_safe_to_expand_and_retry() {
    let mut library = ArtistLibrary::default();
    let mut requests = LibraryRequests::default();
    library.show_more();
    assert!(library.visible_items().is_empty());
    let first = library.begin(&mut requests);
    assert_eq!(
        library
            .complete(first, Err("offline".into()))
            .unwrap()
            .unwrap_err(),
        "offline"
    );
    library.show_more();
    assert!(library.is_empty());
    assert!(library.set_sort(ArtistSort::AlbumCount));
    assert_eq!(library.load.error(), Some("offline"));
    let retry = library.begin(&mut requests);
    assert!(library.load.error().is_none());
    assert!(library
        .complete(retry, Ok(Vec::new()))
        .unwrap()
        .unwrap()
        .is_empty());
    library.show_more();
    assert!(library.visible_items().is_empty());
    assert!(!library.loading());
    load(&mut library, &mut requests, vec![artist("new", Some(1))]);
    assert_eq!(names(library.visible_items()), ["new"]);
}
