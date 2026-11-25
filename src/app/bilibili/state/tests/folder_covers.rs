use super::*;
use std::collections::HashSet;

#[tokio::test]
async fn a_new_list_snapshot_cancels_jobs_and_rejects_old_completions() {
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    let old_snapshot = requests.next();
    state.sync_cover_snapshot(Some(old_snapshot));
    let old = requests.next();
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach_cover("5".into(), old, task.abort_handle());
    state.sync_cover_snapshot(Some(requests.next()));
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!state.finish_cover("5", old));
    assert!(state.needs_cover("5"));
    let current = requests.next();
    let task = tokio::spawn(async {});
    state.attach_cover("5".into(), current, task.abort_handle());
    assert!(!state.finish_cover("5", old));
    assert!(state.finish_cover("5", current));
    assert!(!state.needs_cover("5"));
    task.await.unwrap();
}

#[tokio::test]
async fn changing_visible_folders_cancels_obsolete_work_and_allows_later_retry() {
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    state.sync_cover_snapshot(Some(requests.next()));
    let kept = tokio::spawn(std::future::pending::<()>());
    let cancelled = tokio::spawn(std::future::pending::<()>());
    state.attach_cover("1".into(), requests.next(), kept.abort_handle());
    state.attach_cover("2".into(), requests.next(), cancelled.abort_handle());
    state.prune_cover_jobs(&HashSet::from(["1".into()]));
    assert!(cancelled.await.unwrap_err().is_cancelled());
    assert!(state.needs_cover("2"));
    state.cover_failed(true);
    assert!(kept.await.unwrap_err().is_cancelled());
    assert!(!state.needs_cover("1"));
    assert!(!state.needs_cover("2"));
    assert_eq!(state.cover_failures, 1);
    state.sync_cover_snapshot(Some(requests.next()));
    assert!(state.needs_cover("1"));
    assert_eq!(state.cover_failures, 0);
}

#[test]
fn worker_stop_signal_prevents_new_requests_before_the_ui_receives_the_error() {
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    state.sync_cover_snapshot(Some(requests.next()));
    let old_signal = state.cover_stop_signal();
    assert!(state.needs_cover("1"));
    old_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(!state.needs_cover("1"));
    state.sync_cover_snapshot(Some(requests.next()));
    assert!(state.needs_cover("1"));
    assert!(old_signal.load(std::sync::atomic::Ordering::Relaxed));
    assert!(!state
        .cover_stop_signal()
        .load(std::sync::atomic::Ordering::Relaxed));
}

#[tokio::test]
async fn account_switch_cancels_pending_folder_metadata() {
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    state.begin_history(source("one"), &mut requests);
    state.sync_cover_snapshot(Some(requests.next()));
    let request = requests.next();
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach_cover("same-folder".into(), request, task.abort_handle());
    state.begin_history(source("two"), &mut requests);
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!state.finish_cover("same-folder", request));
    assert_eq!(state.cover_failures, 0);
}
