use super::*;
use crate::app::playlists::search_index::Cancellation;
use crate::playlist::loading::{
    PlaylistFailureKind, PlaylistLoadError, PlaylistProgress, PlaylistUpdate,
};

fn apply_stream(state: &mut PlaylistLibrary, request: LibraryRequest) {
    state.apply_detail_update(request, PlaylistUpdate::Metadata(playlist("p")));
    state.apply_detail_update(
        request,
        PlaylistUpdate::Batch {
            songs: details("p").songs,
            progress: PlaylistProgress {
                songs: 3,
                ..Default::default()
            },
        },
    );
}

fn index() -> Arc<PlaylistSearchIndex> {
    Arc::new(PlaylistSearchIndex::build(&details("p").songs, &Cancellation::default()).unwrap())
}

#[test]
fn index_can_arrive_before_or_after_stream_completion_without_losing_the_query() {
    for before_complete in [true, false] {
        let mut state = PlaylistLibrary::default();
        let mut requests = LibraryRequests::default();
        state.select(playlist("p"));
        state.search.set_query("a".into());
        let request = state.begin_detail(&mut requests).unwrap();
        if before_complete {
            state.accept_search_index(request, index());
        }
        apply_stream(&mut state, request);
        assert!(state.search.start().is_none());
        state.apply_detail_update(
            request,
            PlaylistUpdate::Complete(PlaylistProgress {
                songs: 3,
                ..Default::default()
            }),
        );
        if !before_complete {
            state.accept_search_index(request, index());
        }
        assert!(state.detail_ready());
        assert_eq!(state.search.query, "a");
        let (search_request, index, terms) = state.search.start().unwrap();
        assert!(state.search.complete(
            search_request,
            Ok(index.matching(&terms, &Cancellation::default()).unwrap())
        ));
        assert_eq!(state.search.indices(), Some([0, 2].as_slice()));
        let identity = state.search_identity();
        state.accept_search_index(request, self::index());
        assert_eq!(
            state.search_identity(),
            identity,
            "duplicate index delivery must not invalidate rows"
        );
        assert!(state.search.ready());
    }
}

#[test]
fn stale_indices_are_ignored_after_refresh_reopen_and_source_reset() {
    let mut state = PlaylistLibrary::default();
    let mut requests = LibraryRequests::default();
    state.select(playlist("p"));
    let old = state.begin_detail(&mut requests).unwrap();
    let current = state.begin_detail(&mut requests).unwrap();
    state.accept_search_index(old, index());
    assert!(state.pending_index.is_none());
    state.accept_search_index(current, index());
    assert!(state.pending_index.is_some());
    state.select(playlist("other"));
    state.accept_search_index(current, index());
    assert!(state.pending_index.is_none());
    assert!(state.search.query.is_empty());
    state.select(playlist("p"));
    state.accept_search_index(old, index());
    assert!(state.pending_index.is_none());
    state = PlaylistLibrary::default();
    state.accept_search_index(current, index());
    assert!(state.search.start().is_none());
}

#[test]
fn authorization_failure_removes_search_projection_and_old_index() {
    let mut state = loaded(&mut LibraryRequests::default());
    state.search.set_query("a".into());
    let (request, index, terms) = state.search.start().unwrap();
    state.search.complete(
        request,
        Ok(index.matching(&terms, &Cancellation::default()).unwrap()),
    );
    let mut requests = LibraryRequests::default();
    // Request allocator must survive resets, as it does in NavidromeApp.
    requests.next();
    requests.next();
    let request = state.begin_detail(&mut requests).unwrap();
    state.apply_detail_update(
        request,
        PlaylistUpdate::Failed(PlaylistLoadError {
            kind: PlaylistFailureKind::Authorization,
            message: "denied".into(),
        }),
    );
    assert!(state.songs().is_empty());
    assert!(state.search.indices().is_none());
    assert!(state.search.start().is_none());
    assert!(!state.detail_ready());
}
