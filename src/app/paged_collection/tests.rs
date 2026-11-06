use super::*;

#[test]
fn paging_advances_after_full_and_short_pages_and_stops_only_on_empty_page() {
    let mut collection = PagedCollection::default();
    let mut requests = PageRequests::default();
    assert!(collection.is_empty());
    assert!(!collection.loading());
    assert!(!collection.failed());
    assert!(!collection.exhausted());

    for (page, items) in [(0, (0..100).collect()), (1, vec![100, 101]), (2, vec![])] {
        let request = collection.begin(&mut requests).unwrap();
        assert_eq!(request.page(), page);
        assert!(collection.loading());
        assert!(collection.begin(&mut requests).is_none());
        assert_eq!(
            collection.complete::<&str>(request, Ok(items.clone())),
            Some(Ok(items.as_slice()))
        );
        assert!(!collection.loading());
        assert_eq!(collection.exhausted(), items.is_empty());
    }
    assert_eq!(collection.items(), (0..102).collect::<Vec<_>>());
    assert!(collection.begin(&mut requests).is_none());
}

#[test]
fn failed_page_keeps_existing_items_and_retries_the_same_offset() {
    let mut collection = PagedCollection::default();
    let mut requests = PageRequests::default();
    let first = collection.begin(&mut requests).unwrap();
    collection.complete::<&str>(first, Ok(vec![1, 2]));
    let failed = collection.begin(&mut requests).unwrap();
    assert_eq!(
        collection.complete::<&str>(failed, Err("offline")),
        Some(Err("offline"))
    );
    assert!(collection.failed());
    assert!(!collection.loading());
    assert_eq!(collection.items(), [1, 2]);

    let retry = collection.begin(&mut requests).unwrap();
    assert_eq!(retry.page(), failed.page());
    assert_ne!(retry, failed);
    assert!(!collection.failed());
    assert!(collection.loading());
    assert_eq!(collection.complete::<&str>(failed, Ok(vec![9])), None);
    assert_eq!(collection.complete::<&str>(failed, Err("late error")), None);
    assert!(collection.loading());
    assert_eq!(
        collection.complete::<&str>(retry, Ok(vec![3])),
        Some(Ok(&[3][..]))
    );
    assert_eq!(collection.items(), [1, 2, 3]);
}

#[test]
fn duplicate_responses_cannot_append_twice_or_finish_the_next_request() {
    let mut collection = PagedCollection::default();
    let mut requests = PageRequests::default();
    let first = collection.begin(&mut requests).unwrap();
    collection.complete::<&str>(first, Ok(vec![1]));
    assert_eq!(collection.complete::<&str>(first, Ok(vec![1])), None);
    let second = collection.begin(&mut requests).unwrap();
    assert_eq!(collection.complete::<&str>(first, Ok(vec![])), None);
    assert_eq!(
        collection.complete::<&str>(first, Err("duplicate error")),
        None
    );
    assert!(collection.accepts(second));
    assert!(!collection.exhausted());
    assert!(!collection.failed());
    assert_eq!(collection.items(), [1]);
}

#[test]
fn reset_invalidates_pending_responses_even_before_a_new_request_starts() {
    let mut collection = PagedCollection::default();
    let mut requests = PageRequests::default();
    let old = collection.begin(&mut requests).unwrap();
    collection.reset();
    assert_eq!(collection.complete::<&str>(old, Ok(vec![1])), None);
    assert_eq!(collection.complete::<&str>(old, Err("late error")), None);
    assert!(collection.is_empty());
    assert!(!collection.loading());
    assert!(!collection.failed());
    let current = collection.begin(&mut requests).unwrap();
    assert_eq!(current.page(), 0);
    assert_ne!(old, current);
    assert_eq!(collection.complete::<&str>(old, Ok(vec![1])), None);
    assert_eq!(collection.complete::<&str>(old, Err("late error")), None);
    assert!(collection.accepts(current));
}

#[test]
fn reset_allows_loading_again_after_failure_or_exhaustion() {
    let mut requests = PageRequests::default();
    for result in [Ok(Vec::<i32>::new()), Err("offline")] {
        let mut collection = PagedCollection::default();
        let request = collection.begin(&mut requests).unwrap();
        collection.complete::<&str>(request, result);
        collection.reset();
        assert!(!collection.failed());
        assert!(!collection.exhausted());
        assert_eq!(collection.begin(&mut requests).unwrap().page(), 0);
    }
}

#[test]
fn collections_cannot_accept_each_others_requests() {
    let mut albums = PagedCollection::default();
    let mut genre_albums = PagedCollection::default();
    let mut requests = PageRequests::default();
    let album_request = albums.begin(&mut requests).unwrap();
    let genre_request = genre_albums.begin(&mut requests).unwrap();
    assert_eq!(albums.complete::<&str>(genre_request, Ok(vec![1])), None);
    assert_eq!(
        genre_albums.complete::<&str>(album_request, Ok(vec![2])),
        None
    );
    assert!(albums.accepts(album_request));
    assert!(genre_albums.accepts(genre_request));
}
