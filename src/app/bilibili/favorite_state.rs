//! Favorite dialog ownership survives browsing resets; writes are never aborted here.

use crate::app::LibraryRequest;
use crate::source::{
    bilibili::favorites::{FavoriteOperation, FavoriteOutcome, FavoriteSnapshot},
    SourceId,
};

#[derive(Clone, Debug)]
pub(in crate::app) enum FavoriteDialogKind {
    Picker,
    Remove { folder_id: u64, folder_name: String },
}
#[derive(Clone, Copy)]
pub(in crate::app) enum FavoriteFeedback {
    Saved(bool),
    Checked,
    Uncertain,
}

#[derive(Default)]
pub(in crate::app) struct FavoriteUi {
    pub open: bool,
    pub source: Option<SourceId>,
    pub source_label: String,
    pub bvid: String,
    pub kind: Option<FavoriteDialogKind>,
    pub snapshot: Option<FavoriteSnapshot>,
    pub loading: bool,
    pub writing: bool,
    pub uncertain: bool,
    pub operation: Option<FavoriteOperation>,
    pub feedback: Option<FavoriteFeedback>,
    pub error: Option<String>,
    request: Option<LibraryRequest>,
    read_task: Option<tokio::task::AbortHandle>,
}
impl Drop for FavoriteUi {
    fn drop(&mut self) {
        if let Some(task) = self.read_task.take() {
            task.abort();
        }
    }
}
impl FavoriteUi {
    pub fn begin(
        &mut self,
        source: SourceId,
        label: String,
        bvid: String,
        kind: FavoriteDialogKind,
        request: LibraryRequest,
    ) -> bool {
        if self.writing {
            return false;
        }
        *self = Self::default();
        self.open = true;
        self.source = Some(source);
        self.source_label = label;
        self.bvid = bvid;
        self.kind = Some(kind);
        self.begin_read(request);
        true
    }
    pub fn begin_read(&mut self, request: LibraryRequest) {
        if let Some(task) = self.read_task.take() {
            task.abort();
        }
        self.request = Some(request);
        self.loading = true;
        self.error = None;
    }
    pub fn attach_read(&mut self, task: tokio::task::AbortHandle) {
        self.read_task = Some(task);
    }
    pub fn current(&self, source: &SourceId, request: LibraryRequest) -> bool {
        self.open && self.source.as_ref() == Some(source) && self.request == Some(request)
    }
    pub fn complete_read(
        &mut self,
        source: &SourceId,
        request: LibraryRequest,
        result: Result<FavoriteSnapshot, String>,
    ) -> bool {
        if !self.current(source, request) || self.writing {
            return false;
        }
        self.loading = false;
        self.read_task = None;
        match result {
            Ok(snapshot)
                if snapshot.source == *source
                    && snapshot.video.bvid == self.bvid
                    && snapshot.account_id > 0
                    && self.snapshot.as_ref().is_none_or(|old| {
                        old.account_id == snapshot.account_id && old.video.aid == snapshot.video.aid
                    }) =>
            {
                if self.uncertain {
                    let membership = self
                        .operation
                        .and_then(|operation| snapshot.membership(operation.folder_id));
                    self.uncertain = membership.is_none();
                    self.feedback = Some(
                        if let Some(operation) = self
                            .operation
                            .filter(|operation| membership == Some(operation.present))
                        {
                            FavoriteFeedback::Saved(operation.present)
                        } else if self.uncertain {
                            FavoriteFeedback::Uncertain
                        } else {
                            FavoriteFeedback::Checked
                        },
                    );
                }
                self.snapshot = Some(snapshot);
                self.error = None;
            }
            Ok(_) => self.error = Some("Favorite snapshot identity mismatch".into()),
            Err(error) => self.error = Some(error),
        }
        true
    }
    pub fn begin_write(
        &mut self,
        request: LibraryRequest,
        operation: FavoriteOperation,
    ) -> Option<FavoriteSnapshot> {
        if !self.open || self.loading || self.writing || self.uncertain {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        snapshot.membership(operation.folder_id)?;
        if let Some(FavoriteDialogKind::Picker) = &self.kind {
            if !operation.present {
                return None;
            }
        }
        if let Some(FavoriteDialogKind::Remove { folder_id, .. }) = &self.kind {
            if *folder_id != operation.folder_id || operation.present {
                return None;
            }
        }
        let snapshot = snapshot.clone();
        self.request = Some(request);
        self.operation = Some(operation);
        self.writing = true;
        self.error = None;
        self.feedback = None;
        Some(snapshot)
    }
    pub fn complete_write(
        &mut self,
        source: &SourceId,
        request: LibraryRequest,
        result: Result<FavoriteOutcome, String>,
    ) -> bool {
        if !self.current(source, request) || !self.writing {
            return false;
        }
        self.writing = false;
        match result {
            Ok(FavoriteOutcome::Verified(snapshot)) => {
                if snapshot.source != *source
                    || self.snapshot.as_ref().is_none_or(|old| {
                        old.account_id != snapshot.account_id
                            || old.video.aid != snapshot.video.aid
                            || old.video.bvid != snapshot.video.bvid
                    })
                    || self.operation.is_none_or(|operation| {
                        snapshot.membership(operation.folder_id) != Some(operation.present)
                    })
                {
                    self.error = Some("Favorite outcome identity mismatch".into());
                    self.uncertain = true;
                } else {
                    self.snapshot = Some(snapshot);
                    self.uncertain = false;
                    self.feedback = Some(FavoriteFeedback::Saved(
                        self.operation.is_some_and(|operation| operation.present),
                    ));
                }
            }
            Ok(FavoriteOutcome::Uncertain { snapshot, error }) => {
                self.uncertain = true;
                self.feedback = Some(FavoriteFeedback::Uncertain);
                self.error = Some(error.to_string());
                if snapshot.source == *source
                    && self.snapshot.as_ref().is_some_and(|old| {
                        old.account_id == snapshot.account_id
                            && old.video.aid == snapshot.video.aid
                            && old.video.bvid == snapshot.video.bvid
                    })
                {
                    self.snapshot = Some(snapshot);
                }
            }
            Err(error) => self.error = Some(error),
        }
        true
    }
    pub fn close(&mut self) -> bool {
        if self.writing {
            return false;
        }
        *self = Self::default();
        true
    }
}
#[cfg(test)]
mod tests;
