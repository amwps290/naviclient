use super::*;

#[test]
fn server_scope_is_path_safe_and_stable() {
    assert_eq!(server_scope(Some("a1b2c3d4e5f6")), "a1b2c3d4e5f6");
    assert_eq!(server_scope(Some("AB-CD_ef")), "ab-cd_ef");
    // 空 id 与含非法字符的 id 都退回 default，绝不产生路径分隔符。
    assert_eq!(server_scope(None), "default");
    assert_eq!(server_scope(Some("")), "default");
    assert_eq!(server_scope(Some("../../etc")), "etc");
}

#[test]
fn custom_cache_paths_use_a_dedicated_child_without_changing_the_default() {
    let config = Config {
        cache_dir: Some(PathBuf::from("custom-cache")),
        ..Default::default()
    };
    let managed = PathBuf::from("custom-cache").join(CUSTOM_CACHE_DIRECTORY);
    assert_eq!(audio_cache_dir(&config), managed);
    assert_eq!(cover_cache_dir(&config), managed.join("covers"));
    assert_eq!(
        audio_cache_dir(&Config::default()),
        default_audio_cache_dir()
    );
}

#[test]
fn prepares_a_new_owned_child_and_leaves_legacy_cache_files_in_place() {
    let parent = tempfile::tempdir().unwrap();
    let config = Config {
        cache_dir: Some(parent.path().into()),
        ..Default::default()
    };
    let legacy = parent.path().join("1234567890abcdef1234567890abcdef.audio");
    fs::write(&legacy, b"old cache or user file").unwrap();
    fs::create_dir(parent.path().join("covers")).unwrap();

    let directory = prepare_audio_cache_dir(&config).unwrap();
    assert_eq!(directory, parent.path().join(CUSTOM_CACHE_DIRECTORY));
    assert_eq!(
        fs::read(directory.join(CACHE_OWNER_FILE)).unwrap(),
        CACHE_OWNER
    );
    assert_eq!(prepare_audio_cache_dir(&config).unwrap(), directory);
    assert_eq!(fs::read(legacy).unwrap(), b"old cache or user file");
    assert!(parent.path().join("covers").is_dir());
}

#[test]
fn refuses_to_claim_an_existing_unmanaged_cache_child() {
    let parent = tempfile::tempdir().unwrap();
    let config = Config {
        cache_dir: Some(parent.path().into()),
        ..Default::default()
    };
    let directory = audio_cache_dir(&config);
    fs::create_dir(&directory).unwrap();
    let file = directory.join("1234567890abcdef1234567890abcdef.audio");
    fs::write(&file, b"user file").unwrap();

    assert!(prepare_audio_cache_dir(&config).is_err());
    assert!(!directory.join(CACHE_OWNER_FILE).exists());
    fs::write(directory.join(CACHE_OWNER_FILE), b"not our marker").unwrap();
    assert!(prepare_audio_cache_dir(&config).is_err());
    assert_eq!(fs::read(file).unwrap(), b"user file");
}

#[cfg(unix)]
#[test]
fn refuses_a_symlink_at_the_managed_cache_directory() {
    let parent = tempfile::tempdir().unwrap();
    let config = Config {
        cache_dir: Some(parent.path().into()),
        ..Default::default()
    };
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join(CACHE_OWNER_FILE), CACHE_OWNER).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), audio_cache_dir(&config)).unwrap();
    assert!(prepare_audio_cache_dir(&config).is_err());
}

#[test]
fn source_scopes_keep_legacy_server_paths_and_separate_other_providers() {
    use crate::source::{ProviderKind, SourceId};
    let legacy = SourceId::new(ProviderKind::OpenSubsonic, "Server-A/old");
    let bilibili = SourceId::new(ProviderKind::Bilibili, "Server-A/old");
    assert_eq!(source_scope(&legacy), "server-aold");
    assert_eq!(source_scope(&legacy), server_scope(Some("Server-A/old")));
    assert_ne!(source_scope(&legacy), source_scope(&bilibili));
    assert_ne!(
        source_scope(&bilibili),
        source_scope(&SourceId::new(ProviderKind::Bilibili, "Server-Aold"))
    );
}
