//! WBI query signing. Keys come from the fixed-origin account nav response.

use super::super::client::BilibiliError;
use md5::{Digest, Md5};
use std::collections::BTreeMap;

const ORDER: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

pub(super) fn sign(
    img: &str,
    sub: &str,
    params: &[(&str, String)],
    timestamp: u64,
) -> Result<Vec<(String, String)>, BilibiliError> {
    fn key(value: &str) -> Option<String> {
        let url = url::Url::parse(value).ok()?;
        if url.scheme() != "https"
            || !url.host_str()?.ends_with(".hdslb.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }
        let filename = url.path_segments()?.next_back()?;
        let stem = filename.split_once('.')?.0;
        (stem.len() == 32 && stem.bytes().all(|c| c.is_ascii_hexdigit())).then(|| stem.to_string())
    }
    let combined = format!(
        "{}{}",
        key(img).ok_or(BilibiliError::InvalidResponse)?,
        key(sub).ok_or(BilibiliError::InvalidResponse)?
    );
    let mixed: String = ORDER
        .iter()
        .take(32)
        .map(|i| combined.as_bytes()[*i] as char)
        .collect();
    let mut values = BTreeMap::new();
    for (name, value) in params {
        values.insert(
            (*name).to_string(),
            value
                .chars()
                .filter(|c| !"!'()*".contains(*c))
                .collect::<String>(),
        );
    }
    values.insert("wts".into(), timestamp.to_string());
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(values.iter())
        .finish();
    let digest = format!("{:x}", Md5::digest(format!("{encoded}{mixed}").as_bytes()));
    values.insert("w_rid".into(), digest);
    Ok(values.into_iter().collect())
}
