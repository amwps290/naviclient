use super::*;

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
fn cover_cache_prune_removes_oldest_files_beyond_limit() {
    let dir =
        std::env::temp_dir().join(format!("navidrome-cover-prune-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 三个文件共 300 字节；上限 200：最旧的 two.bin 应被删到回到限额内。
    std::fs::write(dir.join("oldest.cover"), vec![0u8; 150]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("middle.cover"), vec![0u8; 100]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("newest.cover"), vec![0u8; 50]).unwrap();

    let removed = prune_cover_cache_at(&dir, 200);
    assert_eq!(removed, 150);
    assert!(!dir.join("oldest.cover").exists());
    assert!(dir.join("middle.cover").exists());
    assert!(dir.join("newest.cover").exists());

    // 限额内不删除。
    assert_eq!(prune_cover_cache_at(&dir, 200), 0);
    // 目录不存在时静默返回 0。
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(prune_cover_cache_at(&dir, 200), 0);
}
