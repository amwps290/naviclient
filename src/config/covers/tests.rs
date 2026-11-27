use super::*;

#[test]
fn cover_sources_are_isolated_and_open_subsonic_keeps_existing_cache_paths() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        cache_dir: Some(dir.path().into()),
        ..Config::default()
    };
    let subsonic = SourceId::new(ProviderKind::OpenSubsonic, "same-profile");
    let bilibili = SourceId::new(ProviderKind::Bilibili, "same-profile");
    let other = SourceId::new(ProviderKind::Bilibili, "other-account");
    let cover = "../same-cover";
    assert_eq!(
        source_cover_cache_path(&config, &subsonic, cover),
        cover_cache_path(&config, Some("same-profile"), cover)
    );
    write_source_cover_cache(&config, &subsonic, cover, b"subsonic-cover");
    write_source_cover_cache(&config, &bilibili, cover, b"bilibili-cover");
    assert_eq!(
        read_source_cover_cache(&config, &subsonic, cover).unwrap(),
        b"subsonic-cover"
    );
    assert_eq!(
        read_source_cover_cache(&config, &bilibili, cover).unwrap(),
        b"bilibili-cover"
    );
    assert!(read_source_cover_cache(&config, &other, cover).is_none());
    assert_eq!(
        source_cover_cache_path(&config, &bilibili, cover).parent(),
        Some(cover_cache_dir(&config).as_path())
    );
}

#[test]
fn cover_cache_file_name_is_deterministic_and_path_safe() {
    let a = cover_cache_file_name(Some("srv-a"), "al-123");
    let b = cover_cache_file_name(Some("srv-a"), "al-123");
    assert_eq!(a, b);
    assert!(a.ends_with(".cover"));
    // 任何 id（含路径分隔符等危险字符）都映射为纯十六进制名。
    assert_eq!(
        cover_cache_file_name(Some("srv-a"), "../evil/id"),
        cover_cache_file_name(Some("srv-a"), "../evil/id")
    );
    assert!(!cover_cache_file_name(Some("srv-a"), "../evil/id").contains('/'));
    assert_ne!(
        cover_cache_file_name(Some("srv-a"), "al-1"),
        cover_cache_file_name(Some("srv-a"), "al-2")
    );
}

#[test]
fn cover_cache_does_not_cross_servers_with_the_same_id() {
    // 两台服务器都可能返回 id "al-1"，但封面内容不同：缓存键必须区分。
    assert_ne!(
        cover_cache_file_name(Some("srv-a"), "al-1"),
        cover_cache_file_name(Some("srv-b"), "al-1")
    );
}

#[test]
fn cover_cache_round_trips_and_missing_files_miss() {
    let dir = std::env::temp_dir().join(format!("navidrome-cover-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = Config {
        cache_dir: Some(dir.clone()),
        ..Config::default()
    };

    assert!(read_cover_cache(&config, None, "al-404").is_none());
    write_cover_cache(&config, Some("srv-a"), "al-1", b"png-bytes");
    assert_eq!(
        read_cover_cache(&config, Some("srv-a"), "al-1").as_deref(),
        Some(b"png-bytes".as_slice())
    );
    // 另一台服务器读不到这份缓存（同 id 不同内容）。
    assert!(read_cover_cache(&config, Some("srv-b"), "al-1").is_none());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cover_cache_prune_evicts_recognized_names_only() {
    use std::time::{Duration, SystemTime};

    let parent = tempfile::tempdir().unwrap();
    let config = Config {
        cache_dir: Some(parent.path().into()),
        ..Config::default()
    };
    let legacy_covers = parent.path().join("covers");
    fs::create_dir(&legacy_covers).unwrap();
    let legacy = legacy_covers.join(cover_cache_file_name(None, "legacy"));
    fs::write(&legacy, b"leave legacy files untouched").unwrap();
    assert_eq!(
        prune_cover_cache_at(&config, 0).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );

    let subsonic = SourceId::new(ProviderKind::OpenSubsonic, "srv");
    let bilibili = SourceId::new(ProviderKind::Bilibili, "bili");
    for (index, source) in [&subsonic, &bilibili].into_iter().enumerate() {
        write_source_cover_cache(&config, source, "cover", b"cover");
        fs::OpenOptions::new()
            .write(true)
            .open(source_cover_cache_path(&config, source, "cover"))
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(index as u64 + 1))
            .unwrap();
    }
    let user_file = cover_cache_dir(&config).join("my-artwork.cover");
    fs::write(&user_file, b"not app-owned artwork").unwrap();

    assert_eq!(prune_cover_cache_at(&config, 5).unwrap(), 5);
    assert!(!source_cover_cache_path(&config, &subsonic, "cover").exists());
    assert!(source_cover_cache_path(&config, &bilibili, "cover").exists());
    assert_eq!(prune_cover_cache_at(&config, 5).unwrap(), 0);
    assert_eq!(fs::read(user_file).unwrap(), b"not app-owned artwork");
    assert_eq!(fs::read(legacy).unwrap(), b"leave legacy files untouched");
}

#[cfg(unix)]
#[test]
fn cover_cleanup_does_not_follow_a_redirected_cover_directory() {
    let parent = tempfile::tempdir().unwrap();
    let config = Config {
        cache_dir: Some(parent.path().into()),
        ..Config::default()
    };
    prepare_audio_cache_dir(&config).unwrap();
    let other = tempfile::tempdir().unwrap();
    let personal = other.path().join(cover_cache_file_name(None, "outside"));
    fs::write(&personal, b"personal cover").unwrap();
    std::os::unix::fs::symlink(other.path(), cover_cache_dir(&config)).unwrap();

    assert!(prune_cover_cache_at(&config, 0).is_err());
    assert_eq!(fs::read(personal).unwrap(), b"personal cover");
}
