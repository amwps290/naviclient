use super::*;

fn genre(value: &str, count: i64) -> Genre {
    Genre {
        value: value.into(),
        album_count: Some(count),
        song_count: Some(count * 10),
    }
}

fn album(id: &str) -> Album {
    Album {
        id: id.into(),
        cover_art: Some(format!("cover-{id}")),
        ..Default::default()
    }
}

fn names(library: &GenreLibrary) -> Vec<&str> {
    library
        .items()
        .iter()
        .map(|genre| genre.value.as_str())
        .collect()
}

#[test]
fn cache_restore_sorts_by_normalized_name_without_altering_names_or_counts() {
    let mut library = GenreLibrary::default();
    let cached = vec![genre(" rock ", 3), genre("Jazz", 2), genre(" ambient ", 1)];
    library.restore(Some(cached));
    assert_eq!(names(&library), [" ambient ", "Jazz", " rock "]);
    assert_eq!(
        library
            .items()
            .iter()
            .map(|genre| genre.album_count.unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        library
            .items()
            .iter()
            .map(|genre| genre.song_count.unwrap())
            .collect::<Vec<_>>(),
        [10, 20, 30]
    );
    assert!(!library.loading());
    assert!(library.detail().is_none());
    library.restore(None);
    assert!(library.is_empty());
}

#[test]
fn refresh_failures_preserve_cached_list_and_selected_detail_until_retry_succeeds() {
    let mut library = GenreLibrary::default();
    let mut requests = LibraryRequests::default();
    let mut pages = PageRequests::default();
    library.restore(Some(vec![genre("Rock", 3)]));
    library.select("Rock".into());
    let (_, page) = library.begin_albums(&mut pages).unwrap();
    library.complete_albums(page, Ok(vec![album("previous")]));
    let refresh = library.begin_list(&mut requests);
    assert!(library.loading());
    assert_eq!(names(&library), ["Rock"]);
    assert_eq!(
        library
            .complete_list(refresh, Err("offline".into()))
            .unwrap()
            .unwrap_err(),
        "offline"
    );
    assert!(!library.loading());
    assert_eq!(names(&library), ["Rock"]);
    assert_eq!(library.detail().unwrap().albums().items()[0].id, "previous");
    assert!(library
        .complete_list(refresh, Ok(vec![genre("stale", 1)]))
        .is_none());
    assert_eq!(library.load.error(), Some("offline"));

    let retry = library.begin_list(&mut requests);
    assert!(library.load.error().is_none());
    assert!(library
        .complete_list(refresh, Err("late failure".into()))
        .is_none());
    let accepted = library
        .complete_list(retry, Ok(vec![genre("rock", 4), genre("Ambient", 2)]))
        .unwrap()
        .unwrap();
    assert_eq!(
        accepted
            .iter()
            .map(|genre| genre.value.as_str())
            .collect::<Vec<_>>(),
        ["Ambient", "rock"]
    );
    assert_eq!(library.len(), 2);
    assert_eq!(library.detail().unwrap().name(), "Rock");
    assert_eq!(library.detail().unwrap().albums().len(), 1);
}

#[test]
fn only_current_list_success_returns_data_for_caching_and_duplicates_do_not_overwrite_it() {
    let mut library = GenreLibrary::default();
    let mut requests = LibraryRequests::default();
    library.restore(Some(vec![genre("cached", 1)]));
    let old = library.begin_list(&mut requests);
    let current = library.begin_list(&mut requests);
    for result in [Ok(vec![genre("old", 2)]), Err("old error".into())] {
        assert!(library.complete_list(old, result).is_none());
        assert!(library.loading());
        assert_eq!(names(&library), ["cached"]);
    }
    let cacheable = library
        .complete_list(current, Ok(vec![genre("Zulu", 3), genre("alpha", 4)]))
        .unwrap()
        .unwrap();
    assert_eq!(
        cacheable
            .iter()
            .map(|genre| genre.value.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "Zulu"]
    );
    for result in [
        Ok(vec![genre("duplicate", 1)]),
        Err("duplicate failure".into()),
    ] {
        assert!(library.complete_list(current, result).is_none());
        assert_eq!(names(&library), ["alpha", "Zulu"]);
        assert!(library.load.error().is_none());
        assert!(!library.loading());
    }
}

#[test]
fn selecting_another_or_the_same_genre_resets_pages_and_invalidates_previous_responses() {
    let mut library = GenreLibrary::default();
    let mut pages = PageRequests::default();
    library.select("Rock".into());
    let (_, first) = library.begin_albums(&mut pages).unwrap();
    library.complete_albums(first, Ok(vec![album("previous")]));
    let (_, old_rock) = library.begin_albums(&mut pages).unwrap();
    library.select("Jazz".into());
    assert!(library.detail().unwrap().albums().is_empty());
    let (_, old_jazz) = library.begin_albums(&mut pages).unwrap();
    library.complete_albums(old_jazz, Err("failed Jazz page".into()));
    library.select("Rock".into());
    assert!(!library.detail().unwrap().albums().failed());
    let (name, current) = library.begin_albums(&mut pages).unwrap();
    assert_eq!(name, "Rock");
    assert_eq!(current.page(), 0);
    for old in [first, old_rock, old_jazz] {
        assert_eq!(library.complete_albums(old, Ok(vec![album("stale")])), None);
        assert_eq!(
            library.complete_albums(old, Err("stale error".into())),
            None
        );
        assert!(library.detail().unwrap().albums().accepts(current));
    }
    library.select("Rock".into());
    assert_eq!(
        library.complete_albums(current, Ok(vec![album("same genre stale")])),
        None
    );
    let (_, restarted) = library.begin_albums(&mut pages).unwrap();
    assert_ne!(current, restarted);
    assert_eq!(restarted.page(), 0);
    assert_eq!(
        library.complete_albums(restarted, Ok(vec![album("current")])),
        Some(Ok(vec!["cover-current".into()]))
    );
    assert_eq!(library.detail().unwrap().albums().items()[0].id, "current");
}

#[test]
fn paging_requires_selection_retries_same_page_and_stops_only_after_empty_page() {
    let mut library = GenreLibrary::default();
    let mut pages = PageRequests::default();
    assert!(library.begin_albums(&mut pages).is_none());
    library.select(" R&B ".into());
    let (name, first) = library.begin_albums(&mut pages).unwrap();
    assert_eq!(name, " R&B ");
    assert_eq!(first.page(), 0);
    assert!(library.begin_albums(&mut pages).is_none());
    assert_eq!(
        library.complete_albums(first, Ok(vec![album("first")])),
        Some(Ok(vec!["cover-first".into()]))
    );
    let (_, second) = library.begin_albums(&mut pages).unwrap();
    assert_eq!(second.page(), 1);
    assert_eq!(
        library.complete_albums(second, Err("offline".into())),
        Some(Err("offline".into()))
    );
    assert_eq!(library.detail().unwrap().albums().len(), 1);
    assert!(library.detail().unwrap().albums().failed());
    let (_, retry) = library.begin_albums(&mut pages).unwrap();
    assert_eq!(retry.page(), second.page());
    assert_ne!(retry, second);
    assert!(!library.detail().unwrap().albums().failed());
    assert_eq!(
        library.complete_albums(second, Ok(vec![album("stale")])),
        None
    );
    assert_eq!(
        library.complete_albums(retry, Ok(vec![album("retried")])),
        Some(Ok(vec!["cover-retried".into()]))
    );
    let (_, last) = library.begin_albums(&mut pages).unwrap();
    assert_eq!(last.page(), 2);
    assert_eq!(
        library.complete_albums(last, Ok(Vec::new())),
        Some(Ok(Vec::new()))
    );
    assert!(library.detail().unwrap().albums().exhausted());
    assert!(library.begin_albums(&mut pages).is_none());
    assert_eq!(library.detail().unwrap().albums().len(), 2);
}

#[test]
fn pages_preserve_server_order_and_duplicates_and_prefetch_only_new_covers_with_a_cap() {
    let mut library = GenreLibrary::default();
    let mut pages = PageRequests::default();
    library.select("Rock".into());
    let (_, first) = library.begin_albums(&mut pages).unwrap();
    let first_page: Vec<_> = (0..90).rev().map(|id| album(&id.to_string())).collect();
    let covers = library
        .complete_albums(first, Ok(first_page))
        .unwrap()
        .unwrap();
    assert_eq!(covers.len(), 80);
    assert_eq!(covers[0], "cover-89");
    assert_eq!(covers[79], "cover-10");
    let (_, second) = library.begin_albums(&mut pages).unwrap();
    assert_eq!(
        library.complete_albums(first, Ok(vec![album("duplicate response")])),
        None
    );
    assert_eq!(
        library.complete_albums(second, Ok(vec![album("89"), album("new")])),
        Some(Ok(vec!["cover-89".into(), "cover-new".into()]))
    );
    let albums = library.detail().unwrap().albums().items();
    assert_eq!(albums.len(), 92);
    assert_eq!(albums[0].id, "89");
    assert_eq!(albums[89].id, "0");
    assert_eq!(albums[90].id, "89");
    assert_eq!(albums[91].id, "new");
    assert_eq!(
        library.complete_albums(second, Err("duplicate failure".into())),
        None
    );
    assert!(!library.detail().unwrap().albums().failed());
}

#[test]
fn list_and_detail_requests_complete_independently_even_when_selected_genre_disappears() {
    let mut library = GenreLibrary::default();
    let mut requests = LibraryRequests::default();
    let mut pages = PageRequests::default();
    library.restore(Some(vec![genre("Rock", 1)]));
    let list = library.begin_list(&mut requests);
    library.select("Rock".into());
    let (_, page) = library.begin_albums(&mut pages).unwrap();
    assert!(library
        .complete_list(list, Ok(Vec::new()))
        .unwrap()
        .unwrap()
        .is_empty());
    assert!(library.is_empty());
    assert_eq!(library.detail().unwrap().name(), "Rock");
    assert_eq!(
        library.complete_albums(page, Ok(vec![album("rock-album")])),
        Some(Ok(vec!["cover-rock-album".into()]))
    );
    let refresh = library.begin_list(&mut requests);
    library.select("Jazz".into());
    let (_, jazz_page) = library.begin_albums(&mut pages).unwrap();
    assert_eq!(
        library.complete_albums(jazz_page, Err("page failure".into())),
        Some(Err("page failure".into()))
    );
    assert!(library.loading());
    assert!(library
        .complete_list(refresh, Ok(vec![genre("Jazz", 3)]))
        .unwrap()
        .is_ok());
    assert!(library.detail().unwrap().albums().failed());
    assert_eq!(names(&library), ["Jazz"]);
}

#[test]
fn switching_servers_and_restoring_cache_invalidates_old_lists_and_album_pages() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let mut pages = PageRequests::default();
    state.genres.restore(Some(vec![genre("old server", 1)]));
    let old_list = state.genres.begin_list(&mut requests);
    state.genres.select("Rock".into());
    let (_, old_page) = state.genres.begin_albums(&mut pages).unwrap();
    state = AppState::default();
    state.genres.restore(Some(vec![genre("new cached", 2)]));
    let current_list = state.genres.begin_list(&mut requests);
    state.genres.select("Rock".into());
    let (_, current_page) = state.genres.begin_albums(&mut pages).unwrap();
    for result in [
        Ok(vec![genre("old server response", 1)]),
        Err("old-server failure".into()),
    ] {
        assert!(state.genres.complete_list(old_list, result).is_none());
        assert_eq!(names(&state.genres), ["new cached"]);
        assert!(state.genres.loading());
    }
    for result in [
        Ok(vec![album("old server")]),
        Err("old page failure".into()),
    ] {
        assert_eq!(state.genres.complete_albums(old_page, result), None);
        assert!(state
            .genres
            .detail()
            .unwrap()
            .albums()
            .accepts(current_page));
    }
    // Restoring a different cache is also a full reset, even without replacing AppState.
    state.genres.restore(None);
    assert!(state.genres.is_empty());
    assert!(state.genres.detail().is_none());
    assert!(!state.genres.loading());
    assert!(state
        .genres
        .complete_list(current_list, Ok(vec![genre("late list", 1)]))
        .is_none());
    assert_eq!(
        state
            .genres
            .complete_albums(current_page, Ok(vec![album("late page")])),
        None
    );
    let current = state.genres.begin_list(&mut requests);
    assert!(state
        .genres
        .complete_list(current, Ok(vec![genre("current", 3)]))
        .unwrap()
        .is_ok());
    assert_eq!(names(&state.genres), ["current"]);
}
