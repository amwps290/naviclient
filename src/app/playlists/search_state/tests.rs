use super::*;
use crate::app::library::requests::LibraryRequests;
use crate::models::Song;

fn index() -> Arc<PlaylistSearchIndex> {
    Arc::new(
        PlaylistSearchIndex::build(
            &[
                Song {
                    title: "A".into(),
                    ..Default::default()
                },
                Song {
                    title: "B".into(),
                    ..Default::default()
                },
                Song {
                    title: "A".into(),
                    ..Default::default()
                },
            ],
            &Cancellation::default(),
        )
        .unwrap(),
    )
}

fn loaded() -> (PlaylistSearch, LibraryRequest) {
    let snapshot = LibraryRequests::default().next();
    let mut state = PlaylistSearch::default();
    state.replace(snapshot, index());
    (state, snapshot)
}

#[test]
fn search_is_collapsed_by_default_and_query_or_refresh_keeps_it_visible() {
    let mut state = PlaylistSearch::default();
    assert!(!state.expanded);
    state.set_query("a".into());
    assert!(state.expanded);
    let snapshot = LibraryRequests::default().next();
    state.replace(snapshot, index());
    assert!(state.expanded);
    state.set_query(String::new());
    assert!(
        state.expanded,
        "clearing the first Escape must leave input open"
    );
    state = PlaylistSearch::default();
    assert!(!state.expanded, "changing playlists resets the icon entry");
}

#[test]
fn query_before_loading_is_kept_and_equivalent_edits_do_not_invalidate_results() {
    let mut state = PlaylistSearch::default();
    assert!(state.set_query("Ａ".into()));
    assert!(state.start().is_none());
    let snapshot = LibraryRequests::default().next();
    state.replace(snapshot, index());
    let (request, _, _) = state.start().unwrap();
    assert!(state.complete(request, Ok(vec![0, 2])));
    assert!(!state.set_query(" a a ".into()));
    assert!(state.ready());
    assert_eq!(state.indices(), Some([0, 2].as_slice()));
    assert_eq!(state.identity(snapshot), request);
}

#[test]
fn old_query_results_are_rejected_and_clear_is_immediate() {
    let (mut state, _) = loaded();
    state.set_query("a".into());
    let old = state.start().unwrap().0;
    state.set_query("b".into());
    let current = state.start().unwrap().0;
    assert!(!state.complete(old, Ok(vec![0, 2])));
    assert!(state.complete(current, Ok(vec![1])));
    state.set_query("a".into());
    let pending = state.start().unwrap().0;
    state.set_query("　".into());
    assert!(!state.filtered());
    assert!(state.ready());
    assert_eq!(state.indices(), None);
    assert!(!state.complete(pending, Ok(vec![0, 2])));
}

#[test]
fn replacing_snapshot_or_suspending_rejects_late_results() {
    let (mut state, snapshot) = loaded();
    state.set_query("a".into());
    let old = state.start().unwrap().0;
    state.suspend();
    assert!(!state.complete(old, Ok(vec![0, 2])));
    let suspended = state.start().unwrap().0;
    let mut requests = LibraryRequests::default();
    assert_eq!(requests.next(), snapshot);
    let new = requests.next();
    state.replace(new, index());
    assert_eq!(state.query, "a");
    assert!(!state.complete(suspended, Ok(vec![0, 2])));
    let current = state.start().unwrap().0;
    assert!(state.complete(current, Ok(vec![0, 2])));
}

#[test]
fn no_matches_is_ready_but_failure_is_not_and_retry_gets_new_revision() {
    let (mut state, _) = loaded();
    state.set_query("missing".into());
    let request = state.start().unwrap().0;
    assert!(state.complete(request, Ok(vec![])));
    assert!(state.ready());
    assert_eq!(state.indices(), Some([].as_slice()));
    state.set_query("a".into());
    let failed = state.start().unwrap().0;
    assert!(state.complete(failed, Err("worker failed".into())));
    assert!(!state.ready());
    assert_eq!(state.error(), Some("worker failed"));
    assert!(state.start().is_none());
    state.retry();
    let retry = state.start().unwrap().0;
    assert_ne!(retry, failed);
    assert!(!state.complete(failed, Ok(vec![0])));
    assert!(state.complete(retry, Ok(vec![0, 2])));
}

#[test]
fn malformed_projection_is_rejected_and_original_occurrences_map_correctly() {
    for indices in [vec![2, 0], vec![0, 0], vec![3], vec![usize::MAX]] {
        let (mut state, _) = loaded();
        state.set_query("a".into());
        let request = state.start().unwrap().0;
        assert!(state.complete(request, Ok(indices)));
        assert!(state.error().is_some());
        assert!(!state.ready());
    }
    let (mut state, _) = loaded();
    state.set_query("a".into());
    let request = state.start().unwrap().0;
    state.complete(request, Ok(vec![0, 2]));
    assert_eq!(state.original_index(1, 3), Some(2));
    assert_eq!(state.original_index(2, 3), None);
    state.set_query(String::new());
    assert_eq!(state.original_index(1, 3), Some(1));
}

#[tokio::test]
async fn changing_query_cancels_attached_work_cooperatively() {
    let (mut state, _) = loaded();
    state.set_query("a".into());
    let cancel = Cancellation::default();
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach(SearchWork {
        abort: task.abort_handle(),
        cancel: cancel.clone(),
    });
    assert!(state.start().is_none());
    state.set_query("b".into());
    assert!(cancel.cancelled());
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(state.start().is_some());
}
