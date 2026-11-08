use super::*;

#[test]
fn equal_remote_and_profile_ids_remain_distinct_across_providers() {
    let subsonic = SourceId::new(ProviderKind::OpenSubsonic, "account");
    let bilibili = SourceId::new(ProviderKind::Bilibili, "account");
    assert_ne!(subsonic, bilibili);
    assert_ne!(subsonic.cache_namespace(), bilibili.cache_namespace());
    let subsonic_song = MediaKey::new(subsonic, "same-media");
    let bilibili_song = MediaKey::new(bilibili, "same-media");
    assert_ne!(subsonic_song, bilibili_song);
    assert_ne!(subsonic_song.cache_key(), bilibili_song.cache_key());
}

#[test]
fn cache_namespace_preserves_punctuation_case_and_provider_identity() {
    let ids = ["ab", "a/b", "a-b", "a.b", "AB", "../a/b", "账号/音乐"];
    let scopes: std::collections::HashSet<_> = ids
        .into_iter()
        .map(|id| SourceId::new(ProviderKind::Bilibili, id).cache_namespace())
        .collect();
    assert_eq!(scopes.len(), ids.len());
    assert!(scopes.iter().all(|scope| scope
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-')));
}

#[test]
fn media_cache_key_has_unambiguous_component_boundaries() {
    let a = MediaKey::new(SourceId::new(ProviderKind::Bilibili, "a:b"), "c");
    let b = MediaKey::new(SourceId::new(ProviderKind::Bilibili, "a"), "b:c");
    assert_ne!(a.cache_key(), b.cache_key());
    let first_part = MediaKey::new(a.source.clone(), "BV1example:100");
    let second_part = MediaKey::new(a.source, "BV1example:200");
    assert_ne!(first_part.cache_key(), second_part.cache_key());
    let restored: MediaKey =
        serde_json::from_value(serde_json::to_value(&first_part).unwrap()).unwrap();
    assert_eq!(restored.cache_key(), first_part.cache_key());
}

#[test]
fn bilibili_profile_keeps_credential_reference_without_session_material() {
    let profile = SourceProfile::Bilibili {
        id: "stable-profile".into(),
        name: "Music".into(),
        account_id: Some(123),
        credential_key: Some("credentials/stable-profile".into()),
        proxy: None,
    };
    let json = serde_json::to_value(&profile).unwrap();
    assert_eq!(json["provider"], "bilibili");
    assert_eq!(json["credential_key"], "credentials/stable-profile");
    assert!(json.get("cookie").is_none());
    assert!(json.get("password").is_none());
    let restored: SourceProfile = serde_json::from_value(json).unwrap();
    assert_eq!(restored, profile);
    assert_eq!(
        profile.id(),
        SourceId::new(ProviderKind::Bilibili, "stable-profile")
    );
}

#[test]
fn legacy_source_identity_defaults_to_opensubsonic() {
    let restored: SourceId =
        serde_json::from_str(r#"{"profile_id":"existing-server-id"}"#).unwrap();
    assert_eq!(restored.provider, ProviderKind::OpenSubsonic);
    assert_eq!(restored.profile_id, "existing-server-id");
}

#[test]
fn bilibili_first_release_capabilities_only_allow_collection_reads() {
    let bilibili = ProviderKind::Bilibili.capabilities();
    assert!(bilibili.favorite_read);
    assert!(bilibili.playlist_read);
    assert!(!bilibili.favorite_write);
    assert!(!bilibili.playlist_write);
    assert!(!bilibili.library);
    assert!(!bilibili.search);
    assert!(!bilibili.lyrics);
    assert!(!bilibili.recommendations);
    assert!(!bilibili.scrobble);
    assert!(!bilibili.transcoding);
    let subsonic = ProviderKind::OpenSubsonic.capabilities();
    assert!(subsonic.library && subsonic.search && subsonic.scrobble && subsonic.transcoding);
}
