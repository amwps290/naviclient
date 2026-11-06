use super::*;
use crate::app::import::orchestration::ImportUi;
use crate::models::Song;
use tokio::task::JoinHandle;

fn playlist(name: &str, titles: &[&str]) -> ImportedPlaylist {
    ImportedPlaylist {
        name: name.to_string(),
        tracks: titles
            .iter()
            .map(|title| ImportedTrack {
                title: title.to_string(),
                artists: vec!["artist".to_string()],
                album: "album".to_string(),
                duration_ms: 180_000,
            })
            .collect(),
        unavailable_tracks: 2,
    }
}

fn matched(id: &str) -> TrackMatch {
    TrackMatch::Matched(Box::new(Song {
        id: id.to_string(),
        ..Song::default()
    }))
}

fn begin_matching(
    flow: &mut ImportFlow,
    requests: &mut LibraryRequests,
    playlist: ImportedPlaylist,
) -> (LibraryRequest, LibraryRequest) {
    flow.open();
    let resolve = flow.begin_resolve(requests).unwrap();
    let matching = flow
        .complete_resolve(resolve, Ok(playlist), requests)
        .unwrap()
        .unwrap();
    (resolve, matching)
}

async fn pending_task() -> JoinHandle<()> {
    let (started, startup) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = started.send(());
        std::future::pending::<()>().await;
    });
    startup.await.unwrap();
    task
}

#[test]
fn only_url_phase_can_start_resolution_and_each_phase_has_its_own_request() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    assert_eq!(flow.stage(), None);
    assert!(flow.begin_resolve(&mut requests).is_none());

    flow.open();
    flow.set_error("invalid link".into());
    let resolve = flow.begin_resolve(&mut requests).unwrap();
    assert_eq!(flow.stage(), Some(ImportStage::Matching));
    assert!(flow.error().is_none());
    assert!(flow.matching_playlist().is_none());
    assert!(flow.begin_resolve(&mut requests).is_none());
    assert!(flow
        .complete_resolve(
            requests.next(),
            Ok(playlist("wrong", &["wrong"])),
            &mut requests
        )
        .is_none());

    let matching = flow
        .complete_resolve(resolve, Ok(playlist("original", &["first"])), &mut requests)
        .unwrap()
        .unwrap();
    assert_ne!(resolve, matching);
    assert!(flow.begin_resolve(&mut requests).is_none());
    assert!(!flow.complete_match(resolve, Err("wrong phase".into())));
    assert_eq!(flow.matching_playlist().unwrap().name, "original");
    assert!(flow.complete_match(
        matching,
        Ok(MatchedSongs {
            entries: vec![matched("first")]
        })
    ));
    assert_eq!(flow.stage(), Some(ImportStage::Review));
    assert!(flow.begin_resolve(&mut requests).is_none());
}

#[test]
fn resolve_failure_can_retry_without_accepting_previous_responses() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    flow.open();
    let old = flow.begin_resolve(&mut requests).unwrap();
    assert_eq!(
        flow.complete_resolve(old, Err("offline".into()), &mut requests),
        Some(Err("offline"))
    );
    assert_eq!(flow.stage(), Some(ImportStage::Url));
    assert_eq!(flow.error(), Some("offline"));

    let current = flow.begin_resolve(&mut requests).unwrap();
    assert_ne!(old, current);
    assert!(flow.error().is_none());
    for result in [Ok(playlist("old", &["old"])), Err("old failure".into())] {
        assert!(flow.complete_resolve(old, result, &mut requests).is_none());
        assert!(flow.matching_playlist().is_none());
        assert!(flow.error().is_none());
    }
    assert!(flow
        .complete_resolve(current, Ok(playlist("retry", &["new"])), &mut requests)
        .unwrap()
        .is_ok());
    assert_eq!(flow.matching_playlist().unwrap().name, "retry");
}

#[test]
fn concurrent_progress_cannot_regress_overflow_or_update_another_phase() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    let (resolve, matching) = begin_matching(
        &mut flow,
        &mut requests,
        playlist("source", &["a", "b", "c"]),
    );
    flow.update_progress(resolve, 3);
    assert_eq!(flow.progress(), (0, 3));
    flow.update_progress(matching, 2);
    flow.update_progress(matching, 1);
    assert_eq!(flow.progress(), (2, 3));
    flow.update_progress(matching, usize::MAX);
    flow.update_progress(matching, 0);
    assert_eq!(flow.progress(), (3, 3));

    assert!(flow.complete_match(
        matching,
        Ok(MatchedSongs {
            entries: vec![matched("a"), TrackMatch::Failed, TrackMatch::Unmatched]
        })
    ));
    flow.update_progress(matching, 1);
    assert_eq!(flow.progress(), (0, 0));
    assert_eq!(flow.stage(), Some(ImportStage::Review));
    assert_eq!(flow.review().unwrap().rows.len(), 3);
}

#[test]
fn review_keeps_source_order_repeated_matches_and_distinct_outcomes() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    let source = playlist("source", &["a", "missing", "a", "failed", "b"]);
    let source_tracks = source.tracks.clone();
    let (_, matching) = begin_matching(&mut flow, &mut requests, source);
    assert!(flow.complete_match(
        matching,
        Ok(MatchedSongs {
            entries: vec![
                matched("a-id"),
                TrackMatch::Unmatched,
                matched("a-id"),
                TrackMatch::Failed,
                matched("b-id")
            ],
        })
    ));

    let review = flow.review().unwrap();
    assert_eq!(review.name, "source");
    assert_eq!(review.unavailable, 2);
    assert_eq!(
        review
            .rows
            .iter()
            .map(|row| row.source.clone())
            .collect::<Vec<_>>(),
        source_tracks
    );
    assert_eq!(review.song_ids(), ["a-id", "a-id", "b-id"]);
    let counts = review.counts();
    assert_eq!((counts.matched, counts.unmatched, counts.failed), (3, 1, 1));
    assert!(matches!(review.rows[1].outcome, TrackMatch::Unmatched));
    assert!(matches!(review.rows[3].outcome, TrackMatch::Failed));
    assert!(flow.error().is_none());
}

#[test]
fn duplicate_match_success_or_failure_preserves_review_and_save_error() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    let (_, matching) = begin_matching(&mut flow, &mut requests, playlist("original", &["source"]));
    assert!(flow.complete_match(
        matching,
        Ok(MatchedSongs {
            entries: vec![matched("original-id")]
        })
    ));
    flow.set_error("save failed; retry".into());
    for result in [
        Ok(MatchedSongs {
            entries: vec![matched("replacement")],
        }),
        Err("duplicate failure".into()),
    ] {
        assert!(!flow.complete_match(matching, result));
        assert_eq!(flow.stage(), Some(ImportStage::Review));
        assert_eq!(flow.review().unwrap().name, "original");
        assert_eq!(flow.review().unwrap().song_ids(), ["original-id"]);
        assert_eq!(flow.error(), Some("save failed; retry"));
    }
}

#[test]
fn failed_match_discards_source_and_old_results_cannot_affect_retry() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    let (old_resolve, old_matching) =
        begin_matching(&mut flow, &mut requests, playlist("old", &["old"]));
    assert!(flow.complete_match(old_matching, Err("matching failed".into())));
    assert_eq!(flow.stage(), Some(ImportStage::Url));
    assert_eq!(flow.error(), Some("matching failed"));
    assert!(flow.matching_playlist().is_none());
    assert!(flow.review().is_none());
    assert_eq!(flow.progress(), (0, 0));

    let current = flow.begin_resolve(&mut requests).unwrap();
    assert!(flow.error().is_none());
    assert!(flow
        .complete_resolve(
            old_resolve,
            Ok(playlist("stale", &["stale"])),
            &mut requests
        )
        .is_none());
    let matching = flow
        .complete_resolve(current, Ok(playlist("new", &["new"])), &mut requests)
        .unwrap()
        .unwrap();
    for result in [
        Ok(MatchedSongs {
            entries: vec![matched("old")],
        }),
        Err("old failure".into()),
    ] {
        assert!(!flow.complete_match(old_matching, result));
        assert_eq!(flow.matching_playlist().unwrap().name, "new");
        assert!(flow.error().is_none());
    }
    flow.update_progress(old_matching, 1);
    assert_eq!(flow.progress(), (0, 1));
    assert!(flow.complete_match(
        matching,
        Ok(MatchedSongs {
            entries: vec![matched("new-id")]
        })
    ));
    assert_eq!(flow.review().unwrap().song_ids(), ["new-id"]);
}

#[test]
fn malformed_result_lengths_never_create_partial_review() {
    let mut requests = LibraryRequests::default();
    for length in [0, 1, 3] {
        let mut flow = ImportFlow::default();
        let (_, matching) =
            begin_matching(&mut flow, &mut requests, playlist("source", &["a", "b"]));
        assert!(flow.complete_match(
            matching,
            Ok(MatchedSongs {
                entries: vec![matched("id"); length]
            })
        ));
        assert_eq!(flow.stage(), Some(ImportStage::Url));
        assert!(flow.review().is_none());
        assert!(flow.matching_playlist().is_none());
        assert!(flow.error().is_some());
        assert!(!flow.complete_match(
            matching,
            Ok(MatchedSongs {
                entries: vec![matched("a"), matched("b")]
            })
        ));
        assert!(flow.begin_resolve(&mut requests).is_some());
        assert!(flow.error().is_none());
    }
}

#[test]
fn zero_matches_still_enter_review_and_distinguish_failures_from_no_candidates() {
    let mut requests = LibraryRequests::default();
    for (entries, expected_error, unmatched, failed) in [
        (
            vec![TrackMatch::Unmatched, TrackMatch::Unmatched],
            t!("No tracks matched your library.").to_string(),
            2,
            0,
        ),
        (
            vec![TrackMatch::Unmatched, TrackMatch::Failed],
            t!("No tracks are ready to import. Cancel and try again.").to_string(),
            1,
            1,
        ),
    ] {
        let mut flow = ImportFlow::default();
        let (_, matching) =
            begin_matching(&mut flow, &mut requests, playlist("source", &["a", "b"]));
        assert!(flow.complete_match(matching, Ok(MatchedSongs { entries })));
        assert_eq!(flow.stage(), Some(ImportStage::Review));
        assert_eq!(flow.error(), Some(expected_error.as_str()));
        let review = flow.review().unwrap();
        assert!(review.song_ids().is_empty());
        let counts = review.counts();
        assert_eq!(
            (counts.matched, counts.unmatched, counts.failed),
            (0, unmatched, failed)
        );
    }
}

#[test]
fn resetting_ui_and_reopening_same_source_rejects_queued_messages() {
    let mut ui = ImportUi::default();
    let mut requests = LibraryRequests::default();
    let source = playlist("source", &["a"]);
    let (old_resolve, old_matching) = begin_matching(&mut ui.flow, &mut requests, source.clone());
    ui.flow.set_error("old error".into());
    ui.reset();
    assert_eq!(ui.flow.stage(), None);
    assert!(ui.flow.error().is_none());
    assert!(ui
        .flow
        .complete_resolve(old_resolve, Ok(source.clone()), &mut requests)
        .is_none());
    assert!(!ui
        .flow
        .complete_match(old_matching, Err("late failure".into())));
    let (resolve, matching) = begin_matching(&mut ui.flow, &mut requests, source.clone());
    assert_ne!(old_resolve, resolve);
    assert_ne!(old_matching, matching);
    assert!(ui
        .flow
        .complete_resolve(old_resolve, Ok(source), &mut requests)
        .is_none());
    assert!(!ui.flow.complete_match(
        old_matching,
        Ok(MatchedSongs {
            entries: vec![matched("old")]
        })
    ));
    ui.flow.update_progress(old_matching, 1);
    assert_eq!(ui.flow.progress(), (0, 1));
    assert!(ui.flow.complete_match(
        matching,
        Ok(MatchedSongs {
            entries: vec![matched("new")]
        })
    ));
    assert_eq!(ui.flow.review().unwrap().song_ids(), ["new"]);
}

#[tokio::test]
async fn duplicate_resolve_responses_preserve_matching_task_for_cancellation() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    let (resolve, matching) =
        begin_matching(&mut flow, &mut requests, playlist("original", &["a", "b"]));
    let task = pending_task().await;
    flow.set_task(matching, task.abort_handle());
    flow.update_progress(matching, 1);
    for result in [
        Ok(playlist("replacement", &["c"])),
        Err("duplicate failure".into()),
    ] {
        assert!(flow
            .complete_resolve(resolve, result, &mut requests)
            .is_none());
        assert_eq!(flow.matching_playlist().unwrap().name, "original");
        assert_eq!(flow.progress(), (1, 2));
        assert!(flow.error().is_none());
    }
    assert!(!task.is_finished());
    flow.reset();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn reset_and_drop_cancel_live_tasks_in_both_pending_phases() {
    let mut requests = LibraryRequests::default();
    for matching in [false, true] {
        for reset in [false, true] {
            let mut flow = ImportFlow::default();
            flow.open();
            let mut request = flow.begin_resolve(&mut requests).unwrap();
            if matching {
                request = flow
                    .complete_resolve(request, Ok(playlist("source", &["a"])), &mut requests)
                    .unwrap()
                    .unwrap();
            }
            let task = pending_task().await;
            flow.set_task(request, task.abort_handle());
            if reset {
                flow.reset();
                assert_eq!(flow.stage(), None);
            } else {
                drop(flow);
            }
            assert!(task.await.unwrap_err().is_cancelled());
        }
    }
}

#[tokio::test]
async fn stale_task_installation_aborts_only_stale_task_and_replacement_cancels_previous() {
    let mut flow = ImportFlow::default();
    let mut requests = LibraryRequests::default();
    let (resolve, matching) = begin_matching(&mut flow, &mut requests, playlist("source", &["a"]));
    let current = pending_task().await;
    flow.set_task(matching, current.abort_handle());
    let stale = pending_task().await;
    flow.set_task(resolve, stale.abort_handle());
    assert!(stale.await.unwrap_err().is_cancelled());
    assert!(!current.is_finished());

    let replacement = pending_task().await;
    flow.set_task(matching, replacement.abort_handle());
    assert!(current.await.unwrap_err().is_cancelled());
    assert!(!replacement.is_finished());
    flow.reset();
    assert!(replacement.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn resetting_import_ui_aborts_its_flow_task() {
    let mut ui = ImportUi::default();
    let mut requests = LibraryRequests::default();
    ui.flow.open();
    let request = ui.flow.begin_resolve(&mut requests).unwrap();
    let task = pending_task().await;
    ui.flow.set_task(request, task.abort_handle());
    ui.reset();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(ui.flow.stage(), None);
}
