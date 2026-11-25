use super::*;
use crate::models::ListeningEvent;
use crate::source::ProviderKind;

fn source(id: &str) -> SourceId {
    SourceId::new(ProviderKind::Bilibili, id)
}
fn song(id: &str, owner: Option<SourceId>) -> Song {
    Song {
        id: id.into(),
        title: id.into(),
        source: owner,
        ..Default::default()
    }
}
fn history(songs: Vec<Song>) -> ListeningHistory {
    ListeningHistory {
        events: songs
            .into_iter()
            .map(|song| ListeningEvent {
                song,
                started_at: 1,
                listened_seconds: 30,
                completed: false,
            })
            .collect(),
    }
}

#[test]
fn browsing_history_accepts_only_its_source_and_preserves_latest_unique_order() {
    let one = source("one");
    let two = source("two");
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    let request = state.begin_history(one.clone(), &mut requests);
    let data = history(vec![
        song("a", Some(one.clone())),
        song("b", Some(one.clone())),
        song("a", Some(one.clone())),
        song("a", Some(two)),
        song("legacy", None),
    ]);
    assert!(state.complete_history(&one, request, Ok(data)));
    assert_eq!(state.history.events.len(), 3);
    assert_eq!(
        recent_tracks(&state.history, &one)
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}

#[test]
fn old_refreshes_and_previous_accounts_cannot_publish_history() {
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    let one = source("one");
    let old = state.begin_history(one.clone(), &mut requests);
    let current = state.begin_history(one.clone(), &mut requests);
    assert!(!state.complete_history(&one, old, Ok(history(vec![song("old", Some(one.clone()))]))));
    assert!(state.complete_history(
        &one,
        current,
        Ok(history(vec![song("current", Some(one.clone()))]))
    ));
    let two = source("two");
    let next = state.begin_history(two.clone(), &mut requests);
    assert!(state.history.events.is_empty());
    assert!(!state.complete_history(
        &one,
        current,
        Ok(history(vec![song("wrong", Some(one.clone()))]))
    ));
    assert!(state.complete_history(&two, next, Ok(ListeningHistory::default())));
}

#[test]
fn failed_refresh_keeps_known_history_and_exposes_an_error() {
    let one = source("one");
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    let request = state.begin_history(one.clone(), &mut requests);
    state.complete_history(
        &one,
        request,
        Ok(history(vec![song("a", Some(one.clone()))])),
    );
    let refresh = state.begin_history(one.clone(), &mut requests);
    assert!(state.history_loading());
    state.complete_history(&one, refresh, Err("failed".into()));
    assert_eq!(state.history_error(), Some("failed"));
    assert_eq!(state.history.events.len(), 1);
}

#[test]
fn local_folder_search_checks_all_folders_and_normalizes_unicode_terms() {
    let folders = (0..1000)
        .map(|i| Playlist {
            id: i.to_string(),
            name: format!("收藏夹 Music {i}"),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let result = matching_folders(&folders, "收藏夹 ＭＵＳＩＣ 999");
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].id, "999",
        "search must not stop at the initial 50 visible folders"
    );
    assert_eq!(matching_folders(&folders, "　\n").len(), 1000);
    assert!(matching_folders(&folders, "missing").is_empty());
}

#[test]
fn visits_are_bounded_unique_and_queries_reset_paging() {
    let mut state = BilibiliWorkspace::default();
    state.set_folder_query("a".into());
    state.show_more_folders();
    assert_eq!(state.folder_limit, 150);
    state.set_folder_query("b".into());
    assert_eq!(state.folder_limit, 50);
    for i in 0..10 {
        state.visit(&i.to_string());
    }
    assert_eq!(state.visited.len(), 6);
    state.visit("7");
    assert_eq!(state.visited.first().unwrap(), "7");
    assert_eq!(
        state.visited.iter().filter(|id| id.as_str() == "7").count(),
        1
    );
}

#[tokio::test]
async fn switching_sources_cancels_history_and_resets_folder_context() {
    let mut state = BilibiliWorkspace::default();
    let mut requests = LibraryRequests::default();
    state.begin_history(source("one"), &mut requests);
    state.set_folder_query("private".into());
    state.visit("same-id");
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach_history(task.abort_handle());
    state.begin_history(source("two"), &mut requests);
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(state.folder_query.is_empty());
    assert!(state.visited.is_empty());
    assert!(state.history.events.is_empty());
}
