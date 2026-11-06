use super::{build_notification, TrackNotification};
use notify_rust::{Hint, Timeout};

fn request() -> TrackNotification {
    TrackNotification {
        title: "Song".into(),
        body: "Artist — Album".into(),
        cover_path: None,
    }
}

// Build notification payloads only; never call show() or a real D-Bus service.
#[test]
fn notification_payload_preserves_metadata_and_timeout() {
    let notification = build_notification(&request(), None);
    assert_eq!(notification.appname, "Navidrome Client");
    assert_eq!(notification.summary, "Song");
    assert_eq!(notification.body, "Artist — Album");
    assert_eq!(notification.timeout, Timeout::Milliseconds(4000));
    assert!(!notification
        .hints
        .iter()
        .any(|hint| matches!(hint, Hint::ImagePath(_))));
}

#[test]
fn cached_cover_is_used_for_icon_and_large_image() {
    let cover = tempfile::NamedTempFile::new().unwrap();
    let request = TrackNotification {
        cover_path: Some(cover.path().to_path_buf()),
        ..request()
    };
    let notification = build_notification(&request, Some(42));
    let path = cover.path().to_string_lossy().into_owned();
    assert_eq!(notification.icon, path);
    assert!(notification.hints.contains(&Hint::ImagePath(path)));
}

#[test]
fn missing_cover_and_directory_paths_do_not_set_image_hints() {
    let directory = tempfile::tempdir().unwrap();
    for path in [
        directory.path().join("missing.png"),
        directory.path().into(),
    ] {
        let payload = TrackNotification {
            cover_path: Some(path),
            ..request()
        };
        let notification = build_notification(&payload, None);
        assert!(!notification
            .hints
            .iter()
            .any(|hint| matches!(hint, Hint::ImagePath(_))));
        assert_eq!(notification.icon, build_notification(&request(), None).icon);
    }
}
