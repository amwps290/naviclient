use super::*;
use crate::models::{ServerProfile, ShortcutAction, ThemePreference};

fn config() -> Config {
    let mut config = Config {
        server_url: "https://music.example/subsonic".into(),
        username: "listener".into(),
        password: "preserved".into(),
        servers: vec![ServerProfile::new(
            "https://other.example",
            "other",
            "secret",
        )],
        active_server: Some("server".into()),
        theme: ThemePreference::Dark,
        volume: 0.42,
        cache_dir: Some("/custom/audio-cache".into()),
        output_device: Some("device".into()),
        ..Default::default()
    };
    config.shortcuts.set(ShortcutAction::Next, "ctrl-n".into());
    config
        .global_shortcuts
        .set(ShortcutAction::Next, "ctrl-alt-n".into());
    config
}

fn modes() -> impl Iterator<Item = PlayModes> {
    [false, true].into_iter().flat_map(|shuffle| {
        [RepeatMode::Off, RepeatMode::All, RepeatMode::One]
            .into_iter()
            .map(move |repeat| PlayModes { shuffle, repeat })
    })
}

#[test]
fn edits_preserve_independent_shuffle_and_repeat_semantics_for_every_mode() {
    for initial in modes() {
        assert_eq!(
            PlaybackModeEdit::ToggleShuffle.next(initial),
            Some(PlayModes {
                shuffle: !initial.shuffle,
                ..initial
            })
        );
        let next_repeat = match initial.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        assert_eq!(
            PlaybackModeEdit::CycleRepeat.next(initial),
            Some(PlayModes {
                repeat: next_repeat,
                ..initial
            })
        );
        assert_eq!(
            PlaybackModeEdit::AllowContinuation.next(initial),
            (initial.repeat == RepeatMode::One).then_some(PlayModes {
                repeat: RepeatMode::Off,
                ..initial
            })
        );
    }
}

#[test]
fn successful_edits_save_once_from_runtime_modes_and_change_only_the_config_mode_field() {
    for initial in modes() {
        for edit in [
            PlaybackModeEdit::ToggleShuffle,
            PlaybackModeEdit::CycleRepeat,
            PlaybackModeEdit::AllowContinuation,
        ] {
            let Some(next) = edit.next(initial) else {
                continue;
            };
            let mut config = config();
            config.playback_modes = PlayModes {
                shuffle: !initial.shuffle,
                repeat: RepeatMode::Off,
            };
            let mut expected = config.clone();
            expected.playback_modes = next;
            let expected = serde_json::to_value(expected).unwrap();
            let mut runtime = initial;
            let mut saves = 0;

            edit.save(&mut runtime, &mut config, |updated| {
                saves += 1;
                assert_eq!(serde_json::to_value(updated).unwrap(), expected);
                Ok(())
            })
            .unwrap();

            assert_eq!(saves, 1);
            assert_eq!(runtime, next);
            assert_eq!(serde_json::to_value(config).unwrap(), expected);
        }
    }
}

#[test]
fn save_failure_retains_requested_runtime_and_config_modes_and_the_original_error() {
    for initial in modes() {
        for edit in [
            PlaybackModeEdit::ToggleShuffle,
            PlaybackModeEdit::CycleRepeat,
            PlaybackModeEdit::AllowContinuation,
        ] {
            let Some(next) = edit.next(initial) else {
                continue;
            };
            let mut config = config();
            let mut expected = config.clone();
            expected.playback_modes = next;
            let expected = serde_json::to_value(expected).unwrap();
            let mut runtime = initial;
            let mut saves = 0;

            let error = edit
                .save(&mut runtime, &mut config, |updated| {
                    saves += 1;
                    assert_eq!(serde_json::to_value(updated).unwrap(), expected);
                    Err(anyhow::Error::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "read-only preferences",
                    ))
                    .context("mode persistence failed"))
                })
                .unwrap_err();

            assert_eq!(saves, 1);
            assert_eq!(runtime, next);
            assert_eq!(serde_json::to_value(config).unwrap(), expected);
            assert_eq!(
                format!("{error:#}"),
                "mode persistence failed: read-only preferences"
            );
            assert_eq!(
                error.downcast_ref::<std::io::Error>().unwrap().kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
    }
}

#[test]
fn starting_radio_outside_repeat_one_neither_saves_nor_synchronizes_a_different_config_mode() {
    for shuffle in [false, true] {
        for repeat in [RepeatMode::Off, RepeatMode::All] {
            let initial = PlayModes { shuffle, repeat };
            let mut runtime = initial;
            let mut config = config();
            config.playback_modes = PlayModes {
                shuffle: !shuffle,
                repeat: RepeatMode::One,
            };
            let before = serde_json::to_value(&config).unwrap();

            PlaybackModeEdit::AllowContinuation
                .save(&mut runtime, &mut config, |_| {
                    panic!("unchanged radio modes must not save")
                })
                .unwrap();

            assert_eq!(runtime, initial);
            assert_eq!(serde_json::to_value(config).unwrap(), before);
        }
    }
}

#[test]
fn mixed_edits_after_failures_follow_runtime_state_and_preserve_the_last_successful_snapshot() {
    use PlaybackModeEdit::{AllowContinuation, CycleRepeat, ToggleShuffle};
    let mut runtime = PlayModes::default();
    let mut config = config();
    let original = config.clone();
    let mut persisted = config.clone();
    for (edit, fail, writes, expected) in [
        (
            ToggleShuffle,
            false,
            true,
            PlayModes {
                shuffle: true,
                repeat: RepeatMode::Off,
            },
        ),
        (
            CycleRepeat,
            true,
            true,
            PlayModes {
                shuffle: true,
                repeat: RepeatMode::All,
            },
        ),
        (
            CycleRepeat,
            false,
            true,
            PlayModes {
                shuffle: true,
                repeat: RepeatMode::One,
            },
        ),
        (
            AllowContinuation,
            true,
            true,
            PlayModes {
                shuffle: true,
                repeat: RepeatMode::Off,
            },
        ),
        (
            AllowContinuation,
            true,
            false,
            PlayModes {
                shuffle: true,
                repeat: RepeatMode::Off,
            },
        ),
        (
            ToggleShuffle,
            false,
            true,
            PlayModes {
                shuffle: false,
                repeat: RepeatMode::Off,
            },
        ),
        (
            CycleRepeat,
            true,
            true,
            PlayModes {
                shuffle: false,
                repeat: RepeatMode::All,
            },
        ),
        (
            CycleRepeat,
            true,
            true,
            PlayModes {
                shuffle: false,
                repeat: RepeatMode::One,
            },
        ),
        (
            CycleRepeat,
            false,
            true,
            PlayModes {
                shuffle: false,
                repeat: RepeatMode::Off,
            },
        ),
    ] {
        let last_saved = serde_json::to_value(&persisted).unwrap();
        let mut calls = 0;
        let result = edit.save(&mut runtime, &mut config, |updated| {
            calls += 1;
            if fail {
                Err(anyhow::anyhow!("injected failure"))
            } else {
                persisted = updated.clone();
                Ok(())
            }
        });
        assert_eq!(calls, usize::from(writes));
        assert_eq!(result.is_err(), fail && writes);
        assert_eq!(runtime, expected);
        assert_eq!(config.playback_modes, expected);
        let mut expected_config = original.clone();
        expected_config.playback_modes = expected;
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::to_value(expected_config).unwrap()
        );
        if fail || !writes {
            assert_eq!(serde_json::to_value(&persisted).unwrap(), last_saved);
        } else {
            assert_eq!(
                serde_json::to_value(&persisted).unwrap(),
                serde_json::to_value(&config).unwrap()
            );
        }
    }
    let restored: Config =
        serde_json::from_str(&serde_json::to_string(&persisted).unwrap()).unwrap();
    assert_eq!(restored.playback_modes, runtime);
}
