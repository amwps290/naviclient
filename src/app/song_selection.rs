//! Scoped cursor, multi-selection, and range anchors without view calls.

use std::collections::BTreeSet;

use super::song_scope::SongScope;
use super::view_helpers::{range_anchor, selection_range};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MultiSelection {
    pub(super) scope: SongScope,
    pub(super) anchor: usize,
    pub(super) indices: BTreeSet<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SongSelection {
    cursor: Option<(SongScope, usize)>,
    multi: Option<MultiSelection>,
}

impl SongSelection {
    pub(super) fn cursor(&self) -> Option<(SongScope, usize)> {
        self.cursor
    }

    pub(super) fn multi(&self) -> Option<&MultiSelection> {
        self.multi.as_ref()
    }

    pub(super) fn select(&mut self, scope: SongScope, index: usize, len: usize) {
        if index < len {
            self.multi = None;
            self.cursor = Some((scope, index));
        }
    }

    pub(super) fn toggle(&mut self, scope: SongScope, index: usize, len: usize) {
        if index >= len {
            return;
        }
        let mut indices = self
            .multi
            .take()
            .filter(|multi| multi.scope == scope)
            .map_or_else(BTreeSet::new, |multi| multi.indices);
        indices.retain(|index| *index < len);
        if !indices.remove(&index) {
            indices.insert(index);
        }
        self.cursor = Some((scope, index));
        self.multi = (!indices.is_empty()).then_some(MultiSelection {
            scope,
            anchor: index,
            indices,
        });
    }

    pub(super) fn range(&mut self, scope: SongScope, index: usize, len: usize) {
        if index >= len {
            return;
        }
        let anchor = range_anchor(
            self.multi
                .as_ref()
                .filter(|multi| multi.scope == scope && multi.anchor < len)
                .map(|multi| multi.anchor),
            self.cursor
                .filter(|(current, index)| *current == scope && *index < len)
                .map(|(_, index)| index),
            index,
        );
        self.cursor = Some((scope, index));
        self.multi = Some(MultiSelection {
            scope,
            anchor,
            indices: selection_range(anchor, index),
        });
    }

    pub(super) fn move_cursor(
        &mut self,
        scope: SongScope,
        delta: isize,
        len: usize,
    ) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let next = match self.cursor {
            Some((current, index)) if current == scope => {
                index.saturating_add_signed(delta).min(len - 1)
            }
            _ if delta > 0 => 0,
            _ => len - 1,
        };
        self.cursor = Some((scope, next));
        if let Some(multi) = self.multi.as_mut().filter(|multi| multi.scope == scope) {
            multi.anchor = multi.anchor.min(len - 1);
            multi.indices = selection_range(multi.anchor, next);
        }
        Some(next)
    }

    pub(super) fn select_all(&mut self, scope: SongScope, len: usize) {
        if len > 0 {
            self.multi = Some(MultiSelection {
                scope,
                anchor: 0,
                indices: (0..len).collect(),
            });
        }
    }

    pub(super) fn clear_multi(&mut self) {
        self.multi = None;
    }

    pub(super) fn clear_scope(&mut self, scope: SongScope) {
        if self.cursor.is_some_and(|(current, _)| current == scope) {
            self.cursor = None;
        }
        if self
            .multi
            .as_ref()
            .is_some_and(|multi| multi.scope == scope)
        {
            self.multi = None;
        }
    }

    pub(super) fn selected_indices(
        &self,
        scope: SongScope,
        len: usize,
    ) -> impl Iterator<Item = usize> + '_ {
        self.multi
            .as_ref()
            .filter(|multi| multi.scope == scope)
            .into_iter()
            .flat_map(|multi| multi.indices.iter().copied())
            .filter(move |index| *index < len)
    }

    pub(super) fn contains(&self, scope: SongScope, index: usize) -> bool {
        self.multi
            .as_ref()
            .is_some_and(|multi| multi.scope == scope && multi.indices.contains(&index))
    }

    pub(super) fn remap_scope(
        &mut self,
        scope: SongScope,
        old_len: usize,
        new_len: usize,
        map: impl Fn(usize) -> Option<usize>,
    ) {
        let mapped = |index| {
            (index < old_len)
                .then(|| map(index))
                .flatten()
                .filter(|index| *index < new_len)
        };
        if let Some((current, index)) = self.cursor.filter(|(current, _)| *current == scope) {
            self.cursor = mapped(index).map(|index| (current, index));
        }
        if self
            .multi
            .as_ref()
            .is_some_and(|multi| multi.scope == scope)
        {
            let mut multi = self.multi.take().unwrap();
            multi.indices = multi.indices.into_iter().filter_map(mapped).collect();
            if let Some(first) = multi.indices.first().copied() {
                multi.anchor = mapped(multi.anchor).unwrap_or(first);
                self.multi = Some(multi);
            }
        }
    }
}

#[cfg(test)]
mod tests;
