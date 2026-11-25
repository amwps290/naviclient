use super::*;

#[test]
fn cover_metadata_requires_current_list_and_never_erases_a_known_cover() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let snapshot = state.list_snapshot().unwrap();
    assert!(state.apply_folder_cover(
        snapshot,
        "p",
        Some("https://i0.hdslb.com/folder.jpg".into())
    ));
    assert_eq!(
        state.items()[0].cover_art.as_deref(),
        Some("https://i0.hdslb.com/folder.jpg")
    );
    assert!(!state.apply_folder_cover(snapshot, "p", None));
    assert_eq!(
        state.items()[0].cover_art.as_deref(),
        Some("https://i0.hdslb.com/folder.jpg")
    );
    assert!(!state.apply_folder_cover(snapshot, "unknown", Some("bad".into())));
    state.begin_list(&mut requests);
    assert!(!state.apply_folder_cover(snapshot, "p", Some("old".into())));
    assert_eq!(
        state.items()[0].cover_art.as_deref(),
        Some("https://i0.hdslb.com/folder.jpg")
    );
}

#[test]
fn cover_updates_leave_songs_and_detail_snapshot_unchanged() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let snapshot = state.list_snapshot().unwrap();
    let detail = state.snapshot();
    let ids = crate::playlist::song_ids(state.songs());
    state.apply_folder_cover(
        snapshot,
        "p",
        Some("https://i0.hdslb.com/folder.jpg".into()),
    );
    assert_eq!(crate::playlist::song_ids(state.songs()), ids);
    assert_eq!(state.snapshot(), detail);
    assert_eq!(
        state.playlist().unwrap().cover_art.as_deref(),
        Some("https://i0.hdslb.com/folder.jpg")
    );
}
