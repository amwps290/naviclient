use super::*;

fn subtitle(duration: Option<i32>, owner: Option<&str>) -> String {
    playlist_card_subtitle(12, duration, owner)
}

#[test]
fn own_playlist_shows_count_and_duration() {
    let text = subtitle(Some(3900), None);
    assert!(text.contains("12"), "{text}");
    assert!(text.contains(&format_duration(Some(3900))), "{text}");
}

#[test]
fn unknown_duration_is_omitted() {
    let text = subtitle(None, None);
    assert!(text.contains("12"), "{text}");
    assert!(!text.contains('·'), "{text}");
}

#[test]
fn shared_playlist_names_its_owner() {
    let text = subtitle(Some(60), Some("alice"));
    assert!(text.contains("alice"), "{text}");
}
