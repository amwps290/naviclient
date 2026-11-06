use super::*;

fn device(name: &str, is_default: bool) -> OutputDevice {
    OutputDevice {
        name: name.into(),
        is_default,
    }
}

#[test]
fn refreshing_while_loading_keeps_the_pending_request_and_preserves_device_metadata() {
    let mut devices = OutputDevices::default();
    let mut requests = LibraryRequests::default();
    let request = devices.begin(&mut requests).unwrap();
    assert!(devices.begin(&mut requests).is_none());
    assert!(devices.loading());
    let result = vec![device("z speakers", true), device(" USB DAC ", false)];
    assert_eq!(devices.complete(request, Ok(result.clone())), Some(Ok(())));
    assert_eq!(devices.items(), result);
    assert!(!devices.loading());
    assert_eq!(devices.items()[1].name, " USB DAC ");
    assert!(devices.begin(&mut requests).is_some());
}

#[test]
fn failed_refresh_keeps_previous_devices_and_retry_replaces_them() {
    let mut devices = OutputDevices::default();
    let mut requests = LibraryRequests::default();
    let first = devices.begin(&mut requests).unwrap();
    devices.complete(first, Ok(vec![device("Speakers", true)]));
    let refresh = devices.begin(&mut requests).unwrap();
    assert_eq!(devices.items(), [device("Speakers", true)]);
    assert_eq!(
        devices.complete(refresh, Err("device enumeration failed".into())),
        Some(Err("device enumeration failed"))
    );
    assert!(!devices.loading());
    assert_eq!(devices.items(), [device("Speakers", true)]);
    assert_eq!(
        devices.complete(refresh, Ok(vec![device("duplicate", true)])),
        None
    );
    assert_eq!(devices.load.error(), Some("device enumeration failed"));

    let retry = devices.begin(&mut requests).unwrap();
    assert!(devices.load.error().is_none());
    assert_eq!(devices.complete(refresh, Err("late failure".into())), None);
    assert!(devices.loading());
    assert_eq!(
        devices.complete(
            retry,
            Ok(vec![device("Headphones", true), device("Speakers", false)])
        ),
        Some(Ok(()))
    );
    assert_eq!(
        devices.items(),
        [device("Headphones", true), device("Speakers", false)]
    );
}

#[test]
fn duplicate_responses_cannot_replace_devices_or_clear_another_refresh_loading_flag() {
    let mut devices = OutputDevices::default();
    let mut requests = LibraryRequests::default();
    let old = devices.begin(&mut requests).unwrap();
    devices.complete(old, Ok(vec![device("current", true)]));
    for result in [
        Ok(vec![device("duplicate", false)]),
        Err("duplicate error".into()),
    ] {
        assert_eq!(devices.complete(old, result), None);
        assert_eq!(devices.items(), [device("current", true)]);
        assert!(devices.load.error().is_none());
    }
    let current = devices.begin(&mut requests).unwrap();
    for result in [Ok(vec![device("old", true)]), Err("old failure".into())] {
        assert_eq!(devices.complete(old, result), None);
        assert!(devices.loading());
        assert!(devices.begin(&mut requests).is_none());
        assert_eq!(devices.items(), [device("current", true)]);
    }
    assert_eq!(
        devices.complete(current, Ok(vec![device("refreshed", true)])),
        Some(Ok(()))
    );
    assert_eq!(devices.items(), [device("refreshed", true)]);
}

#[test]
fn reconnecting_rejects_old_enumerations_and_errors_without_consuming_new_request() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let old = state.output_devices.begin(&mut requests).unwrap();
    state = AppState::default();
    assert_eq!(
        state
            .output_devices
            .complete(old, Ok(vec![device("old", true)])),
        None
    );
    let current = state.output_devices.begin(&mut requests).unwrap();
    assert_ne!(old, current);
    for result in [Ok(vec![device("old", true)]), Err("old failure".into())] {
        assert_eq!(state.output_devices.complete(old, result), None);
        assert!(state.output_devices.loading());
        assert!(state.output_devices.items().is_empty());
        assert!(state.output_devices.begin(&mut requests).is_none());
    }
    assert_eq!(
        state
            .output_devices
            .complete(current, Ok(vec![device("current", false)])),
        Some(Ok(()))
    );
    assert_eq!(state.output_devices.items(), [device("current", false)]);
}

#[test]
fn initial_failure_and_empty_success_both_allow_later_refresh() {
    let mut devices = OutputDevices::default();
    let mut requests = LibraryRequests::default();
    let first = devices.begin(&mut requests).unwrap();
    assert_eq!(
        devices.complete(first, Err("no audio host".into())),
        Some(Err("no audio host"))
    );
    assert!(!devices.loading());
    assert!(devices.items().is_empty());
    let retry = devices.begin(&mut requests).unwrap();
    assert_eq!(
        devices.complete(retry, Ok(vec![device("USB DAC", true)])),
        Some(Ok(()))
    );
    let refresh = devices.begin(&mut requests).unwrap();
    assert_eq!(devices.complete(refresh, Ok(Vec::new())), Some(Ok(())));
    assert!(devices.items().is_empty());
    assert!(!devices.loading());
    assert!(devices.load.error().is_none());
    let reconnect = devices.begin(&mut requests).unwrap();
    assert_eq!(
        devices.complete(reconnect, Ok(vec![device("reconnected", true)])),
        Some(Ok(()))
    );
}

#[test]
fn device_discovery_and_library_requests_cannot_consume_each_others_responses() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let device_request = state.output_devices.begin(&mut requests).unwrap();
    let search_request = state.search.begin(&mut requests);
    assert_eq!(
        state
            .output_devices
            .complete(search_request, Ok(vec![device("wrong context", false)])),
        None
    );
    assert_eq!(
        state
            .search
            .complete(device_request, Ok(SearchResults::default())),
        None
    );
    assert!(state.output_devices.loading());
    assert!(state.search.loading());
    assert_eq!(
        state
            .output_devices
            .complete(device_request, Ok(vec![device("current", true)])),
        Some(Ok(()))
    );
    assert!(state.search.loading());
    assert_eq!(
        state
            .search
            .complete(search_request, Ok(SearchResults::default())),
        Some(Ok(()))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn blocking_device_enumeration_leaves_the_async_runtime_available() {
    let (started, startup) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let enumeration = tokio::spawn(enumerate_output_devices(move || {
        let _ = started.send(());
        // Bound a regression that runs this blocking call on the async worker:
        // the release callback below would otherwise never get a chance to run.
        released.recv_timeout(Duration::from_secs(5))?;
        Ok(vec![device("enumerated", true)])
    }));
    startup.await.unwrap();
    assert!(!enumeration.is_finished());
    tokio::spawn(async move { release.send(()).unwrap() })
        .await
        .unwrap();
    assert_eq!(
        enumeration.await.unwrap(),
        Ok(vec![device("enumerated", true)])
    );
}

#[tokio::test]
async fn platform_errors_and_worker_panics_complete_loading_and_allow_retry() {
    let mut devices = OutputDevices::default();
    let mut requests = LibraryRequests::default();
    let request = devices.begin(&mut requests).unwrap();
    let error = enumerate_output_devices(|| anyhow::bail!("platform enumeration error")).await;
    assert_eq!(
        devices.complete(request, error),
        Some(Err("platform enumeration error"))
    );
    assert!(!devices.loading());

    let request = devices.begin(&mut requests).unwrap();
    let error = enumerate_output_devices(|| panic!("device enumeration panic"))
        .await
        .unwrap_err();
    assert!(error.contains("device enumeration panic"));
    assert_eq!(
        devices.complete(request, Err(error.clone())),
        Some(Err(error.as_str()))
    );
    assert!(!devices.loading());
    assert!(devices.begin(&mut requests).is_some());
}
