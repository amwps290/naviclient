use super::*;
use crate::app::playlist_dialog::PlaylistDialog;
use crate::app::playlist_library::PlaylistLibrary;
use crate::import::{ImportedPlaylist, ImportedTrack, MatchedSongs, TrackMatch};

fn saved() -> Result<PlaylistEdited, String> {
    Ok(PlaylistEdited {
        id: "saved".into(),
        deleted: false,
    })
}

fn dialog() -> PlaylistUi {
    PlaylistUi {
        dialog: Some(PlaylistDialog::Choose {
            songs: vec!["a".into(), "b".into(), "a".into()],
        }),
        error: Some("previous dialog error".into()),
        ..Default::default()
    }
}

fn review(requests: &mut LibraryRequests) -> ImportUi {
    let mut ui = ImportUi::default();
    ui.flow.open();
    let resolve = ui.flow.begin_resolve(requests).unwrap();
    let matching = ui
        .flow
        .complete_resolve(
            resolve,
            Ok(ImportedPlaylist {
                name: "Imported".into(),
                tracks: ["First", "Second", "First"]
                    .into_iter()
                    .map(|title| ImportedTrack {
                        title: title.into(),
                        artists: vec!["Artist".into()],
                        album: "Album".into(),
                        duration_ms: 180_000,
                    })
                    .collect(),
                unavailable_tracks: 2,
            }),
            requests,
        )
        .unwrap()
        .unwrap();
    assert!(ui.flow.complete_match(
        matching,
        Ok(MatchedSongs {
            entries: ["a", "b", "a"]
                .into_iter()
                .map(|id| {
                    TrackMatch::Matched(Box::new(Song {
                        id: id.into(),
                        ..Default::default()
                    }))
                })
                .collect(),
        }),
    ));
    ui.flow.set_error("previous import error".into());
    ui
}

fn assert_dialog_preserved(ui: &PlaylistUi) {
    let Some(PlaylistDialog::Choose { songs }) = &ui.dialog else {
        panic!("the playlist picker must remain open");
    };
    assert_eq!(songs, &["a", "b", "a"]);
}

fn assert_review_preserved(ui: &ImportUi) {
    let review = ui
        .flow
        .review()
        .expect("import review must remain available");
    assert_eq!(review.name, "Imported");
    assert_eq!(review.unavailable, 2);
    assert_eq!(review.song_ids(), ["a", "b", "a"]);
    assert_eq!(
        review
            .rows
            .iter()
            .map(|row| row.source.title.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second", "First"]
    );
}

#[test]
fn accepted_success_closes_only_the_workflow_that_started_the_write() {
    for origin in [
        PlaylistEditOrigin::Menu,
        PlaylistEditOrigin::Dialog,
        PlaylistEditOrigin::Import,
    ] {
        let mut requests = LibraryRequests::default();
        let mut library = PlaylistLibrary::default();
        let mut playlist_ui = dialog();
        let mut import_ui = review(&mut requests);
        let request = library.begin_edit(&mut requests, origin).unwrap();
        let completion = library.complete_edit(request, saved()).unwrap();
        assert_eq!(completion.origin, origin);
        assert_eq!(completion.result.as_ref().unwrap().id, "saved");
        let close = finish_playlist_workflow(&completion, &mut playlist_ui, &mut import_ui);
        assert_eq!(close, origin != PlaylistEditOrigin::Menu);
        if origin == PlaylistEditOrigin::Dialog {
            assert!(!playlist_ui.dialog_open());
            assert!(playlist_ui.error.is_none());
        } else {
            assert_dialog_preserved(&playlist_ui);
            assert_eq!(playlist_ui.error.as_deref(), Some("previous dialog error"));
        }
        if origin == PlaylistEditOrigin::Import {
            assert!(import_ui.flow.stage().is_none());
            assert!(import_ui.flow.error().is_none());
        } else {
            assert_review_preserved(&import_ui);
            assert_eq!(import_ui.flow.error(), Some("previous import error"));
        }
    }
}

#[test]
fn failed_writes_keep_ordered_data_and_update_only_the_owning_error() {
    for origin in [
        PlaylistEditOrigin::Menu,
        PlaylistEditOrigin::Dialog,
        PlaylistEditOrigin::Import,
    ] {
        let mut requests = LibraryRequests::default();
        let mut library = PlaylistLibrary::default();
        let mut playlist_ui = dialog();
        let mut import_ui = review(&mut requests);
        let request = library.begin_edit(&mut requests, origin).unwrap();
        let completion = library
            .complete_edit(request, Err("save failed".into()))
            .unwrap();
        assert_eq!(completion.origin, origin);
        assert!(!finish_playlist_workflow(
            &completion,
            &mut playlist_ui,
            &mut import_ui,
        ));
        assert!(!library.editing());
        assert_dialog_preserved(&playlist_ui);
        assert_review_preserved(&import_ui);
        assert_eq!(
            playlist_ui.error.as_deref(),
            Some(if origin == PlaylistEditOrigin::Dialog {
                "save failed"
            } else {
                "previous dialog error"
            }),
        );
        assert_eq!(
            import_ui.flow.error(),
            Some(if origin == PlaylistEditOrigin::Import {
                "save failed"
            } else {
                "previous import error"
            }),
        );
    }
}

#[test]
fn competing_writes_cannot_replace_the_pending_workflow_or_invalidate_reads() {
    for origin in [
        PlaylistEditOrigin::Menu,
        PlaylistEditOrigin::Dialog,
        PlaylistEditOrigin::Import,
    ] {
        let mut requests = LibraryRequests::default();
        let mut library = PlaylistLibrary::default();
        let request = library.begin_edit(&mut requests, origin).unwrap();
        let list = library.begin_list(&mut requests);
        for competing in [
            PlaylistEditOrigin::Menu,
            PlaylistEditOrigin::Dialog,
            PlaylistEditOrigin::Import,
        ] {
            assert!(library.begin_edit(&mut requests, competing).is_none());
            assert!(library.list_loading());
            assert!(library.editing());
        }
        assert!(library.complete_edit(list, saved()).is_none());
        assert!(library.editing());
        assert!(library.list_loading());
        let completion = library.complete_edit(request, saved()).unwrap();
        assert_eq!(completion.origin, origin);
        assert!(!library.editing());
        assert!(!library.list_loading());
    }
}

#[test]
fn duplicate_results_cannot_close_a_later_dialog_or_change_its_error() {
    let mut requests = LibraryRequests::default();
    let mut library = PlaylistLibrary::default();
    let old = library
        .begin_edit(&mut requests, PlaylistEditOrigin::Import)
        .unwrap();
    library.complete_edit(old, saved()).unwrap();
    let mut playlist_ui = dialog();
    let mut import_ui = ImportUi::default();
    let current = library
        .begin_edit(&mut requests, PlaylistEditOrigin::Dialog)
        .unwrap();
    for result in [saved(), Err("late import failure".into())] {
        if let Some(completion) = library.complete_edit(old, result) {
            finish_playlist_workflow(&completion, &mut playlist_ui, &mut import_ui);
        }
        assert!(library.editing());
        assert_dialog_preserved(&playlist_ui);
        assert_eq!(playlist_ui.error.as_deref(), Some("previous dialog error"));
        assert!(import_ui.flow.stage().is_none());
    }
    let completion = library.complete_edit(current, saved()).unwrap();
    assert!(finish_playlist_workflow(
        &completion,
        &mut playlist_ui,
        &mut import_ui,
    ));
    assert!(!playlist_ui.dialog_open());
}

#[test]
fn failed_import_can_retry_without_losing_review_or_accepting_old_results() {
    let mut requests = LibraryRequests::default();
    let mut library = PlaylistLibrary::default();
    let mut playlist_ui = PlaylistUi::default();
    let mut import_ui = review(&mut requests);
    let failed = library
        .begin_edit(&mut requests, PlaylistEditOrigin::Import)
        .unwrap();
    let completion = library
        .complete_edit(failed, Err("offline".into()))
        .unwrap();
    assert!(!finish_playlist_workflow(
        &completion,
        &mut playlist_ui,
        &mut import_ui,
    ));
    assert_review_preserved(&import_ui);
    assert_eq!(import_ui.flow.error(), Some("offline"));
    let retry = library
        .begin_edit(&mut requests, PlaylistEditOrigin::Import)
        .unwrap();
    import_ui.flow.clear_error();
    assert!(import_ui.flow.error().is_none());
    assert_review_preserved(&import_ui);
    assert!(library.complete_edit(failed, saved()).is_none());
    assert!(library.editing());
    let completion = library.complete_edit(retry, saved()).unwrap();
    assert_eq!(completion.origin, PlaylistEditOrigin::Import);
    assert!(finish_playlist_workflow(
        &completion,
        &mut playlist_ui,
        &mut import_ui,
    ));
    assert!(import_ui.flow.stage().is_none());
}

#[test]
fn success_after_dialog_reset_does_not_request_an_unrelated_window_close() {
    for origin in [PlaylistEditOrigin::Dialog, PlaylistEditOrigin::Import] {
        let mut requests = LibraryRequests::default();
        let mut library = PlaylistLibrary::default();
        let request = library.begin_edit(&mut requests, origin).unwrap();
        let completion = library.complete_edit(request, saved()).unwrap();
        let mut playlist_ui = PlaylistUi::default();
        let mut import_ui = ImportUi::default();
        assert!(!finish_playlist_workflow(
            &completion,
            &mut playlist_ui,
            &mut import_ui,
        ));
    }
}
