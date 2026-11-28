//! Ordered metadata search, independent of UI, network and playback.

use crate::models::Song;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Default)]
pub(in crate::app) struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Aborting an async waiter also asks its already-running blocking work to stop.
pub(in crate::app) struct CancelOnDrop(pub Cancellation);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn normalize(value: &str) -> String {
    value.nfkc().flat_map(char::to_lowercase).collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct QueryTerms(Vec<String>);
impl QueryTerms {
    pub fn new(query: &str) -> Self {
        let mut terms: Vec<_> = normalize(query)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        terms.sort();
        terms.dedup();
        Self(terms)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Default)]
pub(crate) struct PlaylistSearchIndex {
    fields: Vec<Vec<String>>,
}

impl PlaylistSearchIndex {
    #[cfg(test)]
    pub(in crate::app) fn build(songs: &[Song], cancel: &Cancellation) -> Option<Self> {
        let mut index = Self::default();
        index.append(songs, cancel)?;
        Some(index)
    }

    /// Build incrementally on the loading worker, before handing each owned
    /// batch to the UI. No second Song vector or UI-thread normalization.
    pub(in crate::app) fn append(&mut self, songs: &[Song], cancel: &Cancellation) -> Option<()> {
        self.fields.reserve(songs.len());
        for song in songs {
            if cancel.cancelled() {
                return None;
            }
            let mut values = vec![
                song.title.as_str(),
                song.artist.as_str(),
                song.album.as_str(),
            ];
            if let Some(part) = &song.bilibili_part {
                values.extend([
                    part.uploader.as_str(),
                    part.video_title.as_str(),
                    part.title.as_str(),
                    part.collection_title.as_str(),
                ]);
            }
            let mut values: Vec<_> = values
                .into_iter()
                .filter(|v| !v.trim().is_empty())
                .map(normalize)
                .collect();
            values.sort();
            values.dedup();
            self.fields.push(values);
        }
        (!cancel.cancelled()).then_some(())
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub(in crate::app) fn approx_bytes(&self) -> usize {
        let outer = self.fields.capacity() * std::mem::size_of::<Vec<String>>();
        let inner: usize = self
            .fields
            .iter()
            .map(|row| {
                row.capacity() * std::mem::size_of::<String>()
                    + row.iter().map(String::capacity).sum::<usize>()
            })
            .sum();
        outer.saturating_add(inner)
    }

    pub(in crate::app) fn matching(
        &self,
        terms: &QueryTerms,
        cancel: &Cancellation,
    ) -> Option<Vec<usize>> {
        let mut indices = Vec::new();
        for (index, fields) in self.fields.iter().enumerate() {
            if index % 128 == 0 && cancel.cancelled() {
                return None;
            }
            if terms
                .0
                .iter()
                .all(|term| fields.iter().any(|field| field.contains(term)))
            {
                indices.push(index);
            }
        }
        (!cancel.cancelled()).then_some(indices)
    }
}

#[cfg(test)]
mod tests;
