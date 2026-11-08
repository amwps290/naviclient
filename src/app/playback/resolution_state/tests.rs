use super::*;
use crate::source::ProviderKind;

fn media(profile: &str) -> MediaKey {
    MediaKey::new(
        SourceId::new(ProviderKind::OpenSubsonic, profile),
        "same-id",
    )
}

#[test]
fn newer_intent_rejects_old_success_or_error_without_consuming_current() {
    let mut requests = LibraryRequests::default();
    let mut state = ResolutionState::default();
    let occurrence = Some(PlaybackId::new());
    let old = state.begin(media("a"), occurrence, "old", &mut requests);
    let current = state.begin(media("a"), occurrence, "current", &mut requests);
    assert!(state.complete(&old, occurrence).is_none());
    assert!(state.pending());
    assert_eq!(state.complete(&current, occurrence), Some("current"));
    assert!(state.complete(&current, occurrence).is_none());
}

#[test]
fn cancelled_or_changed_occurrence_cannot_commit_even_with_same_media() {
    let mut requests = LibraryRequests::default();
    let mut state = ResolutionState::default();
    let old_occurrence = Some(PlaybackId::new());
    let old = state.begin(media("a"), old_occurrence, 1, &mut requests);
    state.cancel();
    let current = state.begin(media("a"), old_occurrence, 2, &mut requests);
    assert!(state.complete(&old, old_occurrence).is_none());
    assert!(state.complete(&current, Some(PlaybackId::new())).is_none());
    assert!(!state.pending());
}

#[test]
fn browsing_another_source_does_not_cancel_playback_but_logout_does() {
    let mut requests = LibraryRequests::default();
    let mut state = ResolutionState::default();
    let owner = media("playing");
    let request = state.begin(owner.clone(), None, "audio", &mut requests);
    state.cancel_source(&media("browsing").source);
    assert!(state.pending());
    assert_eq!(state.complete(&request, None), Some("audio"));
    let request = state.begin(owner.clone(), None, "audio", &mut requests);
    state.cancel_source(&owner.source);
    assert!(state.complete(&request, None).is_none());
}

#[test]
fn failed_preload_is_attempted_once_per_source_and_occurrence() {
    let mut requests = LibraryRequests::default();
    let mut state = ResolutionState::default();
    let occurrence = Some(PlaybackId::new());
    let key = media("a");
    let request = state.begin(key.clone(), occurrence, (), &mut requests);
    state.complete(&request, occurrence);
    assert!(state.attempted(&key, occurrence));
    assert!(!state.attempted(&media("b"), occurrence));
    assert!(!state.attempted(&key, Some(PlaybackId::new())));
    state.cancel();
    assert!(!state.attempted(&key, occurrence));
}

#[test]
fn cancellation_aborts_an_inflight_future_and_releases_its_resources() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut requests = LibraryRequests::default();
        let mut state = ResolutionState::default();
        let request = state.begin(media("a"), None, (), &mut requests);
        let (held, released) = tokio::sync::oneshot::channel::<()>();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _held = held;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        state.attach(&request, task.abort_handle());
        ready.await.unwrap();
        state.cancel();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(released.await.is_err());
        assert!(state.complete(&request, None).is_none());
    });
}
