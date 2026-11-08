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
fn custom_audio_cache_directories_own_the_cover_subdirectory() {
    let config = Config {
        cache_dir: Some(PathBuf::from("custom-cache")),
        ..Default::default()
    };
    assert_eq!(audio_cache_dir(&config), PathBuf::from("custom-cache"));
    assert_eq!(
        cover_cache_dir(&config),
        PathBuf::from("custom-cache").join("covers")
    );
    let default_config = Config::default();
    assert_eq!(audio_cache_dir(&default_config), default_audio_cache_dir());
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
