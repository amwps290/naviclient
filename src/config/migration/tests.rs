use super::*;

#[test]
fn legacy_playback_mode_field_migrates_to_playback_modes() {
    let mut value: serde_json::Value =
        serde_json::from_str(r#"{"server_url":"http://x","playback_mode":"repeat_one"}"#).unwrap();
    migrate_legacy_playback_mode(&mut value);
    assert_eq!(value["playback_modes"]["shuffle"], serde_json::json!(false));
    assert_eq!(value["playback_modes"]["repeat"], "one");

    let mut value: serde_json::Value =
        serde_json::from_str(r#"{"playback_mode":"shuffle"}"#).unwrap();
    migrate_legacy_playback_mode(&mut value);
    assert_eq!(value["playback_modes"]["shuffle"], serde_json::json!(true));
    assert_eq!(value["playback_modes"]["repeat"], "off");
}

#[test]
fn existing_playback_modes_win_over_legacy_field() {
    let mut value: serde_json::Value = serde_json::from_str(
        r#"{"playback_modes":{"shuffle":true,"repeat":"all"},"playback_mode":"sequential"}"#,
    )
    .unwrap();
    migrate_legacy_playback_mode(&mut value);
    assert_eq!(value["playback_modes"]["repeat"], "all");
    assert_eq!(value["playback_modes"]["shuffle"], serde_json::json!(true));
}
