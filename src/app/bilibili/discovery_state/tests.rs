use super::*;
use crate::models::Song;
use crate::source::ProviderKind;

fn source(id: &str) -> SourceId {
    SourceId::new(ProviderKind::Bilibili, id)
}
fn page(source: &SourceId, keyword: &str, page: u32) -> SearchPage {
    SearchPage {
        source: source.clone(),
        keyword: keyword.into(),
        page,
        pages: 3,
        total: 0,
        videos: Vec::new(),
    }
}
fn video(source: &SourceId) -> VideoTracks {
    VideoTracks {
        source: source.clone(),
        title: "Video".into(),
        uploader: "UP".into(),
        cover: None,
        songs: vec![Song {
            id: "BV1GJ411x7h7:1".into(),
            source: Some(source.clone()),
            ..Default::default()
        }],
        selected: 0,
    }
}

#[test]
fn new_queries_and_accounts_reject_old_search_results() {
    let mut state = DiscoveryState::default();
    let mut requests = LibraryRequests::default();
    let owner = source("one");
    let old = state.begin_search(&owner, "a".into(), 1, &mut requests);
    let current = state.begin_search(&owner, "b".into(), 1, &mut requests);
    assert!(!state.complete_search(&owner, old, Ok(page(&owner, "a", 1))));
    assert!(state.complete_search(&owner, current, Ok(page(&owner, "b", 1))));
    assert_eq!(state.results.as_ref().unwrap().keyword, "b");
    state.draft = "private old query".into();
    state.bind(&source("two"));
    assert!(state.results.is_none());
    assert!(state.draft.is_empty());
    assert!(!state.complete_search(&owner, current, Ok(page(&owner, "b", 1))));
}

#[test]
fn paging_failure_preserves_last_page_and_retry_cannot_be_overwritten() {
    let mut state = DiscoveryState::default();
    let mut requests = LibraryRequests::default();
    let owner = source("one");
    let first = state.begin_search(&owner, "a".into(), 1, &mut requests);
    state.complete_search(&owner, first, Ok(page(&owner, "a", 1)));
    let failed = state.begin_search(&owner, "a".into(), 2, &mut requests);
    state.complete_search(&owner, failed, Err("network".into()));
    assert_eq!(state.results.as_ref().unwrap().page, 1);
    assert_eq!(state.search_error(), Some("network"));
    let retry = state.begin_search(&owner, "a".into(), 2, &mut requests);
    assert!(!state.complete_search(&owner, failed, Ok(page(&owner, "a", 2))));
    assert!(state.complete_search(&owner, retry, Ok(page(&owner, "a", 2))));
    assert_eq!(state.results.as_ref().unwrap().page, 2);
}

#[test]
fn previews_keep_source_identity_and_close_invalidates_playback_ticket() {
    let mut state = DiscoveryState::default();
    let mut requests = LibraryRequests::default();
    let owner = source("one");
    let old = state.begin_video(&owner, "av42".into(), &mut requests);
    let current = state.begin_video(&owner, "av43".into(), &mut requests);
    assert!(!state.complete_video(&owner, old, Ok(video(&owner))));
    assert!(state.complete_video(&owner, current, Ok(video(&owner))));
    assert_eq!(state.video_request, Some(current));
    let (songs, index) = state.playback_plan(&owner, current, 0).unwrap();
    assert_eq!(index, 0);
    assert_eq!(songs[0].source.as_ref(), Some(&owner));
    assert!(state.playback_plan(&source("two"), current, 0).is_none());
    assert!(state.playback_plan(&owner, old, 0).is_none());
    assert!(state.playback_plan(&owner, current, 1).is_none());
    state.close_video();
    assert!(state.playback_plan(&owner, current, 0).is_none());
    assert!(state.video_request.is_none());
    assert!(state.video.is_none());
    assert!(!state.complete_video(&owner, current, Ok(video(&owner))));
}

#[test]
fn foreign_video_parts_and_incorrect_search_snapshots_are_not_accepted() {
    let mut state = DiscoveryState::default();
    let mut requests = LibraryRequests::default();
    let owner = source("one");
    let request = state.begin_video(&owner, "av42".into(), &mut requests);
    let mut wrong = video(&owner);
    wrong.songs[0].source = Some(source("two"));
    assert!(state.complete_video(&owner, request, Ok(wrong)));
    assert!(state.video.is_none());
    assert!(state.video_error().is_some());
    let request = state.begin_search(&owner, "a".into(), 1, &mut requests);
    state.complete_search(&owner, request, Ok(page(&owner, "other", 2)));
    assert!(state.results.is_none());
    assert!(state.search_error().is_some());
}

#[tokio::test]
async fn input_rejection_source_switch_and_new_queries_cancel_owned_tasks() {
    let mut state = DiscoveryState::default();
    let mut requests = LibraryRequests::default();
    let owner = source("one");
    state.begin_video(&owner, "av42".into(), &mut requests);
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach_video(task.abort_handle());
    state.begin_search(&owner, "a".into(), 1, &mut requests);
    assert!(task.await.unwrap_err().is_cancelled());
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach_search(task.abort_handle());
    state.reject_input("invalid input".into());
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(state.input_error.as_deref(), Some("invalid input"));
    assert!(state.video_request.is_none());
    state.begin_search(&owner, "a".into(), 1, &mut requests);
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach_search(task.abort_handle());
    state.bind(&source("two"));
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!state.search_loading());
}
