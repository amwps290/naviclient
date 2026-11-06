use super::*;
use std::sync::mpsc;

fn tray() -> (LinuxTray, mpsc::Receiver<TrayCommand>) {
    let (tx, rx) = mpsc::channel();
    (LinuxTray::new(tx), rx)
}

#[test]
fn tray_identity_menu_order_and_playback_labels_remain_stable() {
    let (mut tray, rx) = tray();
    assert_eq!(tray.id(), "navidrome-client");
    assert_eq!(tray.title(), "Navidrome Client");
    for (paused, label, icon) in [
        (false, "Pause", "media-playback-pause"),
        (true, "Play", "media-playback-start"),
        (false, "Pause", "media-playback-pause"),
    ] {
        tray.set_paused(paused);
        let menu = tray.menu();
        assert_eq!(menu.len(), 6);
        assert!(matches!(menu[3], MenuItem::Separator));
        let items: Vec<_> = menu
            .iter()
            .filter_map(|item| match item {
                MenuItem::Standard(item) => Some(item),
                _ => None,
            })
            .collect();
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            [label, "Previous", "Next", "Show window", "Exit"]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.icon_name.as_str())
                .collect::<Vec<_>>(),
            [
                icon,
                "media-skip-backward",
                "media-skip-forward",
                "window-new",
                "application-exit"
            ]
        );
        assert!(items.iter().all(|item| item.enabled && item.visible));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn captured_menu_and_left_click_forward_each_command_once_without_changing_playback_state() {
    let (mut tray, rx) = tray();
    for paused in [false, true] {
        tray.set_paused(paused);
        let menu = tray.menu();
        // A host can invoke an older menu after the app has published a new
        // playback state. Its callbacks still forward the same commands.
        tray.set_paused(!paused);
        for item in menu {
            if let MenuItem::Standard(item) = item {
                (item.activate)(&mut tray);
            }
        }
        tray.activate(-1, 999);
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            [
                TrayCommand::TogglePlayback,
                TrayCommand::Previous,
                TrayCommand::Next,
                TrayCommand::ShowWindow,
                TrayCommand::Quit,
                TrayCommand::TogglePlayback,
            ]
        );
        assert_eq!(tray.paused, !paused);
    }
}

#[test]
fn callbacks_silently_tolerate_a_disconnected_app_channel() {
    let (mut tray, rx) = tray();
    drop(rx);
    for paused in [false, true] {
        tray.set_paused(paused);
        for item in tray.menu() {
            if let MenuItem::Standard(item) = item {
                (item.activate)(&mut tray);
            }
        }
        tray.activate(0, 0);
        assert_eq!(tray.paused, paused);
    }
}

#[test]
fn both_icon_states_publish_complete_independent_pixmaps_at_all_supported_sizes() {
    let (mut tray, _) = tray();
    let playing = tray.icon_pixmap();
    tray.set_paused(true);
    let paused = tray.icon_pixmap();
    for pixmaps in [&playing, &paused] {
        assert_eq!(
            pixmaps
                .iter()
                .map(|icon| (icon.width, icon.height))
                .collect::<Vec<_>>(),
            [(16, 16), (22, 22), (24, 24), (32, 32), (48, 48)]
        );
        for icon in pixmaps {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
            assert!(icon.data.chunks_exact(4).any(|pixel| pixel[0] > 0));
        }
    }
    assert!(playing
        .iter()
        .zip(&paused)
        .all(|(playing, paused)| playing.data != paused.data));

    let mut published = tray.icon_pixmap();
    published[0].data.fill(0);
    for is_paused in [true, true, false, false] {
        tray.set_paused(is_paused);
        let expected = if is_paused { &paused } else { &playing };
        assert!(tray
            .icon_pixmap()
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.data == expected.data));
    }
}
