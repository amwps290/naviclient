use super::*;

#[test]
fn displays_original_stream_metadata() {
    let song = Song {
        suffix: Some("flac".to_string()),
        bit_rate: Some(941),
        size: Some(25_165_824),
        duration: Some(214),
        ..Song::default()
    };

    let info = playback_technical_info(&song, TranscodingQuality::Original);

    assert_eq!(info.format, "FLAC");
    assert_eq!(info.tooltip, "Current stream: FLAC · 941 kbps · 24.0 MB");
}

#[test]
fn displays_transcoded_stream_profile_and_estimated_size() {
    let song = Song {
        suffix: Some("flac".to_string()),
        size: Some(25_165_824),
        duration: Some(240),
        ..Song::default()
    };

    let info = playback_technical_info(&song, TranscodingQuality::Kbps192);

    assert_eq!(info.format, "MP3");
    assert_eq!(
        info.tooltip,
        "Current stream: MP3 · 192 kbps · ~5.5 MB\nSource file: FLAC · ~838 kbps · 24.0 MB"
    );
}

#[test]
fn formats_file_sizes_for_compact_player_display() {
    assert_eq!(format_file_size(800), "800 B");
    assert_eq!(format_file_size(1_536), "1.5 KB");
    assert_eq!(format_file_size(3 * 1024 * 1024), "3.0 MB");
}
