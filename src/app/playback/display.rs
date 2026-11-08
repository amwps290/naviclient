//! Playback technical-information and file-size formatting.

use rust_i18n::t;

use crate::models::{Song, TranscodingQuality};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct PlaybackTechnicalInfo {
    pub(in crate::app) format: String,
    pub(in crate::app) tooltip: String,
}

pub(in crate::app) fn bilibili_technical_info(
    quality: Option<&crate::source::AudioQuality>,
) -> PlaybackTechnicalInfo {
    PlaybackTechnicalInfo {
        format: "AAC-LC".into(),
        tooltip: quality
            .and_then(|quality| quality.bandwidth)
            .map(|bandwidth| {
                t!(
                    "Independent AAC-LC audio · declared bandwidth %{bandwidth} bit/s",
                    bandwidth = bandwidth
                )
                .to_string()
            })
            .unwrap_or_else(|| {
                t!("Independent audio · quality will be resolved on playback").to_string()
            }),
    }
}

pub(in crate::app) fn playback_technical_info(
    song: &Song,
    quality: TranscodingQuality,
) -> PlaybackTechnicalInfo {
    let source = source_technical_info(song);
    if let Some(bit_rate) = quality.max_bit_rate() {
        let estimated_bytes = song.duration.and_then(|seconds| {
            let seconds = u64::try_from(seconds).ok()?;
            Some(
                seconds
                    .saturating_mul(u64::from(bit_rate))
                    .saturating_mul(1_000)
                    / 8,
            )
        });
        let size = estimated_bytes
            .map(|bytes| format!("~{}", format_file_size(bytes)))
            .unwrap_or_else(|| t!("Unknown size").to_string());
        let bit_rate = format!("{bit_rate} kbps");
        let display = t!(
            "MP3 · %{bit_rate} · %{size}",
            bit_rate = bit_rate,
            size = size
        )
        .to_string();
        return PlaybackTechnicalInfo {
            format: "MP3".to_string(),
            tooltip: t!(
                "Current stream: %{display}\nSource file: %{source}",
                display = display,
                source = source.display
            )
            .to_string(),
        };
    }

    PlaybackTechnicalInfo {
        format: source.format,
        tooltip: t!("Current stream: %{display}", display = source.display).to_string(),
    }
}

struct SourceTechnicalInfo {
    format: String,
    display: String,
}

fn source_technical_info(song: &Song) -> SourceTechnicalInfo {
    let format = song
        .suffix
        .as_deref()
        .filter(|suffix| !suffix.trim().is_empty())
        .map(|suffix| suffix.trim().to_ascii_uppercase())
        .or_else(|| {
            song.content_type
                .as_deref()
                .and_then(format_from_content_type)
        })
        .unwrap_or_else(|| "AUDIO".to_string());
    let bit_rate = song
        .bit_rate
        .and_then(|bit_rate| u32::try_from(bit_rate).ok())
        .filter(|bit_rate| *bit_rate > 0)
        .map(|bit_rate| format!("{bit_rate} kbps"))
        .or_else(|| estimated_bit_rate(song).map(|bit_rate| format!("~{bit_rate} kbps")))
        .unwrap_or_else(|| t!("Unknown rate").to_string());
    let size = song
        .size
        .and_then(|size| u64::try_from(size).ok())
        .map(format_file_size)
        .unwrap_or_else(|| t!("Unknown size").to_string());
    let display = format!("{format} · {bit_rate} · {size}");

    SourceTechnicalInfo { format, display }
}

fn format_from_content_type(content_type: &str) -> Option<String> {
    let subtype = content_type
        .split(';')
        .next()?
        .trim()
        .rsplit('/')
        .next()?
        .trim_start_matches("x-");
    if subtype.is_empty() {
        return None;
    }
    Some(match subtype.to_ascii_lowercase().as_str() {
        "mpeg" | "mp3" => "MP3".to_string(),
        "mp4" | "aac" => "AAC".to_string(),
        "ogg" | "vorbis" => "OGG".to_string(),
        other => other.to_ascii_uppercase(),
    })
}

fn estimated_bit_rate(song: &Song) -> Option<u64> {
    let bytes = u64::try_from(song.size?).ok()?;
    let seconds = u64::try_from(song.duration?).ok()?;
    (seconds > 0).then(|| bytes.saturating_mul(8) / seconds / 1_000)
}

pub(in crate::app) fn format_file_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.2} GB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests;
