//! Conservative matching: Unicode is retained, versions and duration are strict.

use super::provider::{Query, Record};
use unicode_normalization::UnicodeNormalization;

pub(crate) const VERSION: u32 = 1;

pub(crate) fn normalize(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn versions(value: &str) -> Vec<&'static str> {
    let value = normalize(value);
    [
        "live",
        "现场",
        "現場",
        "remix",
        "cover",
        "翻唱",
        "instrumental",
        "伴奏",
        "acoustic",
        "radioedit",
        "demo",
        "演唱会",
        "演唱會",
        "unplugged",
        "karaoke",
        "spedup",
        "slowed",
    ]
    .into_iter()
    .filter(|tag| value.contains(tag))
    .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MatchInfo {
    pub names: bool,
    pub version: bool,
    pub duration_delta: Option<f64>,
    pub album: Option<bool>,
    pub trusted: bool,
}

pub(crate) fn assess(query: &Query, record: &Record) -> MatchInfo {
    let names = !query.title.trim().is_empty()
        && !query.artist.trim().is_empty()
        && normalize(&query.title) == normalize(&record.track_name)
        && normalize(&query.artist) == normalize(&record.artist_name);
    let version = versions(&format!("{} {}", query.title, query.album))
        == versions(&format!("{} {}", record.track_name, record.album_name));
    let duration_delta = query
        .duration
        .filter(|d| (1..=3600).contains(d))
        .and_then(|duration| {
            (record.duration.is_finite() && (1.0..=3600.0).contains(&record.duration))
                .then(|| (f64::from(duration) - record.duration).abs())
        });
    let album = (!query.album.trim().is_empty() && !record.album_name.trim().is_empty())
        .then(|| normalize(&query.album) == normalize(&record.album_name));
    MatchInfo {
        names,
        version,
        duration_delta,
        album,
        trusted: names && version && duration_delta.is_some_and(|d| d <= 2.0) && record.usable(),
    }
}

pub(crate) fn ranked(query: &Query, mut records: Vec<Record>) -> Vec<Record> {
    records.retain(Record::usable);
    records.sort_by(|a, b| {
        let a = assess(query, a);
        let b = assess(query, b);
        b.trusted
            .cmp(&a.trusted)
            .then(b.names.cmp(&a.names))
            .then(b.version.cmp(&a.version))
            .then(b.album.cmp(&a.album))
            .then_with(|| {
                a.duration_delta
                    .unwrap_or(f64::MAX)
                    .total_cmp(&b.duration_delta.unwrap_or(f64::MAX))
            })
    });
    let mut unique: Vec<Record> = Vec::new();
    for record in records {
        if !unique.iter().any(|r| {
            normalize(&r.track_name) == normalize(&record.track_name)
                && normalize(&r.artist_name) == normalize(&record.artist_name)
                && versions(&r.album_name) == versions(&record.album_name)
                && (r.duration - record.duration).abs() <= 2.0
                && r.instrumental == record.instrumental
                && r.lyrics().lines == record.lyrics().lines
        }) {
            unique.push(record);
        }
    }
    unique.truncate(20);
    unique
}

pub(crate) fn automatic(query: &Query, records: Vec<Record>) -> Result<Option<Record>, ()> {
    let trusted: Vec<_> = ranked(query, records)
        .into_iter()
        .filter(|r| assess(query, r).trusted)
        .collect();
    // Different texts/timelines remain ambiguous even if album metadata ranks one first.
    if trusted.len() > 1 {
        Err(())
    } else {
        Ok(trusted.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query() -> Query {
        Query {
            title: "Ｈｅｌｌｏ".into(),
            artist: "Adèle".into(),
            duration: Some(223),
            ..Default::default()
        }
    }
    fn record() -> Record {
        Record {
            track_name: "hello".into(),
            artist_name: "Ade\u{300}le".into(),
            duration: 224.0,
            synced_lyrics: Some("[00:01]test".into()),
            ..Default::default()
        }
    }
    #[test]
    fn unicode_versions_and_durations() {
        assert_eq!(normalize("稻香 · 周杰倫"), "稻香周杰倫");
        assert_ne!(normalize("周杰倫"), normalize("周杰伦"));
        assert!(assess(&query(), &record()).trusted);
        for r in [
            Record {
                duration: 0.0,
                ..record()
            },
            Record {
                duration: 226.0,
                ..record()
            },
            Record {
                album_name: "Live".into(),
                ..record()
            },
        ] {
            assert!(!assess(&query(), &r).trusted);
        }
        assert!(
            !assess(
                &Query {
                    artist: String::new(),
                    ..query()
                },
                &record()
            )
            .trusted
        );
    }
    #[test]
    fn duplicates_fold_but_different_timelines_need_selection() {
        assert!(automatic(
            &query(),
            vec![
                record(),
                Record {
                    id: 2,
                    album_name: "Compilation".into(),
                    ..record()
                }
            ]
        )
        .unwrap()
        .is_some());
        assert!(automatic(
            &query(),
            vec![
                record(),
                Record {
                    synced_lyrics: Some("[00:02]test".into()),
                    ..record()
                }
            ]
        )
        .is_err());
    }
}
