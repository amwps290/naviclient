use super::*;
use crate::app::library::requests::LibraryRequests;
use crate::source::{
    bilibili::favorites::{FavoriteFolderChoice, FavoriteVideo},
    ProviderKind,
};

fn source(id: &str) -> SourceId {
    SourceId::new(ProviderKind::Bilibili, id)
}
fn snapshot(source: &SourceId, contains: bool) -> FavoriteSnapshot {
    FavoriteSnapshot {
        source: source.clone(),
        account_id: 123,
        video: FavoriteVideo {
            bvid: "BV1GJ411x7h7".into(),
            aid: 42,
            title: "Video".into(),
        },
        folders: vec![FavoriteFolderChoice {
            id: 5,
            title: "Music".into(),
            contains: Some(contains),
        }],
    }
}
fn prepared(kind: FavoriteDialogKind) -> (FavoriteUi, LibraryRequests, SourceId) {
    let mut state = FavoriteUi::default();
    let mut requests = LibraryRequests::default();
    let owner = source("one");
    let request = requests.next();
    state.begin(
        owner.clone(),
        "Account one".into(),
        "BV1GJ411x7h7".into(),
        kind,
        request,
    );
    state.complete_read(&owner, request, Ok(snapshot(&owner, false)));
    (state, requests, owner)
}

#[test]
fn read_results_require_current_dialog_source_request_and_video() {
    let (mut state, mut requests, owner) = prepared(FavoriteDialogKind::Picker);
    let old = requests.next();
    state.begin_read(old);
    let current = requests.next();
    state.begin_read(current);
    assert!(!state.complete_read(&owner, old, Ok(snapshot(&owner, true))));
    assert!(!state.complete_read(&source("two"), current, Ok(snapshot(&owner, true))));
    assert!(state.complete_read(&owner, current, Ok(snapshot(&owner, true))));
    assert!(state.close());
    assert!(!state.complete_read(&owner, current, Ok(snapshot(&owner, false))));
}

#[test]
fn picker_cannot_remove_and_removal_cannot_address_another_folder() {
    let (mut state, mut requests, _) = prepared(FavoriteDialogKind::Picker);
    assert!(state
        .begin_write(
            requests.next(),
            FavoriteOperation {
                folder_id: 5,
                present: false
            }
        )
        .is_none());
    assert!(state
        .begin_write(
            requests.next(),
            FavoriteOperation {
                folder_id: 99,
                present: true
            }
        )
        .is_none());
    let (mut state, mut requests, _) = prepared(FavoriteDialogKind::Remove {
        folder_id: 5,
        folder_name: "Music".into(),
    });
    assert!(state
        .begin_write(
            requests.next(),
            FavoriteOperation {
                folder_id: 6,
                present: false
            }
        )
        .is_none());
    assert!(state
        .begin_write(
            requests.next(),
            FavoriteOperation {
                folder_id: 5,
                present: true
            }
        )
        .is_none());
}

#[test]
fn accepted_write_cannot_be_closed_or_replaced_by_another_source() {
    let (mut state, mut requests, owner) = prepared(FavoriteDialogKind::Picker);
    let request = requests.next();
    assert!(state
        .begin_write(
            request,
            FavoriteOperation {
                folder_id: 5,
                present: true
            }
        )
        .is_some());
    assert!(!state.close());
    assert!(!state.begin(
        source("two"),
        "two".into(),
        "BV1GJ411x7h7".into(),
        FavoriteDialogKind::Picker,
        requests.next()
    ));
    assert!(!state.complete_write(
        &source("two"),
        request,
        Ok(FavoriteOutcome::Verified(snapshot(&owner, true)))
    ));
    assert!(state.complete_write(
        &owner,
        request,
        Ok(FavoriteOutcome::Verified(snapshot(&owner, true)))
    ));
    assert!(!state.writing);
    assert!(!state.uncertain);
    assert!(state.close());
}

#[test]
fn uncertain_write_blocks_resubmission_until_a_read_checks_status() {
    let (mut state, mut requests, owner) = prepared(FavoriteDialogKind::Picker);
    let request = requests.next();
    state.begin_write(
        request,
        FavoriteOperation {
            folder_id: 5,
            present: true,
        },
    );
    let mut unknown = snapshot(&owner, false);
    unknown.folders[0].contains = None;
    state.complete_write(
        &owner,
        request,
        Ok(FavoriteOutcome::Uncertain {
            snapshot: unknown,
            error: crate::source::bilibili::client::BilibiliError::Network,
        }),
    );
    assert!(state.uncertain);
    assert!(state
        .begin_write(
            requests.next(),
            FavoriteOperation {
                folder_id: 5,
                present: true
            }
        )
        .is_none());
    let check = requests.next();
    state.begin_read(check);
    state.complete_read(&owner, check, Ok(snapshot(&owner, true)));
    assert!(!state.uncertain);
    assert!(state.error.is_none());
    assert_eq!(state.snapshot.as_ref().unwrap().membership(5), Some(true));
}

#[test]
fn mismatched_outcome_is_not_reported_as_saved() {
    let (mut state, mut requests, owner) = prepared(FavoriteDialogKind::Picker);
    let request = requests.next();
    state.begin_write(
        request,
        FavoriteOperation {
            folder_id: 5,
            present: true,
        },
    );
    let mut wrong = snapshot(&owner, true);
    wrong.account_id = 124;
    state.complete_write(&owner, request, Ok(FavoriteOutcome::Verified(wrong)));
    assert!(state.uncertain);
    assert!(state.error.is_some());
}

#[tokio::test]
async fn closing_during_read_cancels_only_the_read_task() {
    let (mut state, _, _) = prepared(FavoriteDialogKind::Picker);
    let task = tokio::spawn(std::future::pending::<()>());
    state.attach_read(task.abort_handle());
    assert!(state.close());
    assert!(task.await.unwrap_err().is_cancelled());
}
