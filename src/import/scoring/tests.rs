use super::{
    artist_score, best_match, duration_score, normalize, strip_parenthetical, title_score,
};
use crate::import::ImportedTrack;
use crate::models::Song;

fn song(title: &str, artist: &str, secs: i32) -> Song {
    Song {
        title: title.to_string(),
        artist: artist.to_string(),
        duration: Some(secs),
        ..Song::default()
    }
}

fn track(title: &str, artists: &[&str], ms: u32) -> ImportedTrack {
    ImportedTrack {
        title: title.to_string(),
        artists: artists.iter().map(|artist| artist.to_string()).collect(),
        album: String::new(),
        duration_ms: ms,
    }
}

#[test]
fn normalize_folds_width_case_and_punctuation() {
    assert_eq!(normalize(" Ｏｎｅ Ｄａｙ "), "oneday");
    assert_eq!(normalize("晴天（Live）"), "晴天live");
    assert_eq!(normalize("A.B.C"), "abc");
}

#[test]
fn strip_parenthetical_removes_trailing_group_only() {
    assert_eq!(strip_parenthetical("晴天 (Live)"), "晴天");
    assert_eq!(strip_parenthetical("夜曲【伴奏】"), "夜曲");
    assert_eq!(strip_parenthetical("Song (A) (B)"), "Song (A) (B)");
    assert_eq!(strip_parenthetical("Intro (The End)"), "Intro (The End)");
    assert_eq!(strip_parenthetical("无括号"), "无括号");
    // 不平衡：结尾闭括号没有对应开括号，保持原样。
    assert_eq!(strip_parenthetical("奇怪）"), "奇怪）");
}

#[test]
fn title_score_covers_equality_containment_and_fuzzy() {
    assert_eq!(title_score("晴天", "晴天"), 1.0);
    // 结尾括号后缀剥掉后全等。
    assert_eq!(title_score("晴天(Live)", "晴天"), 1.0);
    // 互含但主体不等。
    assert_eq!(title_score("晴天", "晴天的回忆"), 0.9);
    // 纯编辑距离封顶 0.7，不越过接受线。
    let fuzzy = title_score("晴天", "睛天");
    assert!(fuzzy > 0.0 && fuzzy <= 0.7, "fuzzy score was {fuzzy}");
    assert_eq!(title_score("晴天", "晚安"), 0.0);
}

#[test]
fn artist_score_splits_candidates_and_matches_any_imported() {
    let imported = vec!["周杰伦".to_string(), "杨瑞代".to_string()];
    assert_eq!(artist_score(&imported, "周杰伦"), 1.0);
    assert_eq!(artist_score(&imported, "周杰伦 / 杨瑞代"), 1.0);
    assert_eq!(artist_score(&imported, "Someone"), 0.0);
    assert_eq!(artist_score(&[], "周杰伦"), 0.0);
    assert_eq!(artist_score(&["A".to_string()], "ABBA"), 0.0);
}

#[test]
fn duration_score_rewards_close_lengths() {
    assert_eq!(duration_score(200_000, Some(200)), 1.0);
    assert_eq!(duration_score(200_000, Some(202)), 1.0);
    assert!(duration_score(200_000, Some(230)) < 0.5);
    assert_eq!(duration_score(200_000, None), 0.5);
    assert_eq!(duration_score(0, Some(200)), 0.5);
}

#[test]
fn best_match_accepts_strong_hits_and_rejects_impostors() {
    let imported = track("晴天", &["周杰伦"], 269_000);
    let library = vec![
        song("晴天", "周杰伦", 269),
        song("晴天", "张三", 269),     // 同名同时长，但艺人不符，不能过线
        song("晴天线", "周杰伦", 269), // 标题不过线
        song("晚安", "周杰伦", 269),   // 完全不同歌
    ];
    let matched = best_match(&imported, &library).expect("should match");
    assert_eq!(matched.title, "晴天");
    assert_eq!(matched.artist, "周杰伦");

    // 同名翻唱即使时长完全相同，艺人硬门槛也会拒绝。
    let wrong_artist = vec![song("晴天", "路人甲", 269)];
    assert!(best_match(&imported, &wrong_artist).is_none());

    // 客串艺人命中不能代替主艺人，否则会自动选中错误版本。
    let collaboration = track("晴天", &["周杰伦", "杨瑞代"], 269_000);
    let guest_only = vec![song("晴天", "杨瑞代", 269)];
    assert!(best_match(&collaboration, &guest_only).is_none());

    // 同名翻唱 + 时长相差 40 秒同样拒绝。
    let impostors = vec![song("晴天", "路人甲", 305)];
    assert!(best_match(&imported, &impostors).is_none());
}

#[test]
fn empty_candidates_match_nothing() {
    let imported = track("晴天", &["周杰伦"], 269_000);
    assert!(best_match(&imported, &[]).is_none());
}
