use super::*;

#[test]
fn atomic_json_write_replaces_complete_documents_without_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    std::fs::write(&path, r#"{"old":true}"#).unwrap();

    let replacement = serde_json::json!({"queue": ["a", "b"], "position": 42});
    write_json_atomic(&path, &replacement, "test state").expect("atomic write should succeed");

    let restored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored, replacement);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

struct SerializationFailure;

impl serde::Serialize for SerializationFailure {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("synthetic serialization failure"))
    }
}

#[test]
fn serialization_failure_leaves_existing_files_and_missing_directories_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let original = b"{\"old\":true}";
    fs::write(&path, original).unwrap();
    assert!(write_json_atomic(&path, &SerializationFailure, "test state").is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    let missing = dir.path().join("missing").join("state.json");
    assert!(write_json_atomic(&missing, &SerializationFailure, "test state").is_err());
    assert!(!dir.path().join("missing").exists());
}

#[test]
fn replacement_failure_keeps_the_destination_and_cleans_up_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("state.json");
    fs::create_dir(&destination).unwrap();
    let original = destination.join("existing");
    fs::write(&original, "preserved").unwrap();
    assert!(write_json_atomic(
        &destination,
        &serde_json::json!({"new": true}),
        "test state"
    )
    .is_err());
    assert_eq!(fs::read_to_string(&original).unwrap(), "preserved");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn missing_malformed_and_wrongly_typed_json_are_cache_misses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.json");
    assert!(read_json_at::<Vec<String>>(&path).is_none());
    for text in ["{broken", "{}", "null", "true"] {
        fs::write(&path, text).unwrap();
        assert!(read_json_at::<Vec<String>>(&path).is_none());
    }
    fs::write(&path, r#"["song-1","song-2"]"#).unwrap();
    assert_eq!(
        read_json_at::<Vec<String>>(&path).unwrap(),
        ["song-1", "song-2"]
    );
}
