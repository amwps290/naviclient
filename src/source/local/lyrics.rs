//! Local lyrics loader: prefers sibling `.lrc` / `.txt` files, then embedded tags.

use std::fs;
use std::path::Path;

use anyhow::Result;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::ItemKey;

use crate::lyrics::lrc;
use crate::models::Lyrics;

pub(super) fn load_local_lyrics(audio_path: &Path) -> Result<Lyrics> {
    // 1. Sibling `.lrc` file (case-insensitive extension fallback).
    for ext in ["lrc", "LRC", "txt", "TXT"] {
        let candidate = audio_path.with_extension(ext);
        if candidate.is_file() {
            if let Ok(bytes) = fs::read(&candidate) {
                let text = decode_text_bytes(&bytes);
                let parsed = parse_text_lyrics(&text);
                if !parsed.lines.is_empty() {
                    return Ok(parsed);
                }
            }
        }
    }

    // 2. Embedded lyrics tag via lofty.
    if let Ok(tagged) = Probe::open(audio_path).and_then(|probe| probe.read()) {
        for tag in tagged.tags() {
            if let Some(text) = tag.get_string(ItemKey::Lyrics) {
                let parsed = parse_text_lyrics(text);
                if !parsed.lines.is_empty() {
                    return Ok(parsed);
                }
            }
        }
    }

    Ok(Lyrics::default())
}

fn parse_text_lyrics(text: &str) -> Lyrics {
    let synced = lrc::parse(text);
    if !synced.lines.is_empty() {
        synced
    } else {
        lrc::plain(text)
    }
}

fn decode_text_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}
