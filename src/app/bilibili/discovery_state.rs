//! Request ownership for Bilibili search pages and video-part previews.

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::source::{
    bilibili::discovery::{SearchPage, VideoTracks},
    SourceId,
};

#[derive(Default)]
pub(in crate::app) struct DiscoveryState {
    source: Option<SourceId>,
    pub draft: String,
    pub keyword: String,
    pub page: u32,
    pub results: Option<SearchPage>,
    pub video: Option<VideoTracks>,
    pub video_request: Option<LibraryRequest>,
    pub video_input: String,
    pub input_error: Option<String>,
    search_load: LibraryLoad,
    video_load: LibraryLoad,
    search_task: Option<tokio::task::AbortHandle>,
    video_task: Option<tokio::task::AbortHandle>,
}
impl Drop for DiscoveryState {
    fn drop(&mut self) {
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        if let Some(task) = self.video_task.take() {
            task.abort();
        }
    }
}
impl DiscoveryState {
    pub fn bind(&mut self, source: &SourceId) {
        if self.source.as_ref() != Some(source) {
            *self = Self::default();
            self.source = Some(source.clone());
        }
    }
    pub fn begin_search(
        &mut self,
        source: &SourceId,
        keyword: String,
        page: u32,
        requests: &mut LibraryRequests,
    ) -> LibraryRequest {
        self.bind(source);
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        self.close_video();
        if self.keyword != keyword {
            self.results = None;
        }
        self.keyword = keyword;
        self.page = page;
        self.input_error = None;
        self.search_load.begin(requests)
    }
    pub fn attach_search(&mut self, task: tokio::task::AbortHandle) {
        self.search_task = Some(task);
    }
    pub fn complete_search(
        &mut self,
        source: &SourceId,
        request: LibraryRequest,
        result: Result<SearchPage, String>,
    ) -> bool {
        if self.source.as_ref() != Some(source) {
            return false;
        }
        let result = result.and_then(|page| {
            if page.source != *source || page.keyword != self.keyword || page.page != self.page {
                Err("invalid search snapshot".into())
            } else {
                Ok(page)
            }
        });
        let Some(result) = self.search_load.complete(request, result) else {
            return false;
        };
        self.search_task = None;
        if let Ok(page) = result {
            self.results = Some(page);
        }
        true
    }
    pub fn begin_video(
        &mut self,
        source: &SourceId,
        input: String,
        requests: &mut LibraryRequests,
    ) -> LibraryRequest {
        self.bind(source);
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        self.search_load = Default::default();
        self.close_video();
        self.input_error = None;
        self.video_input = input;
        let request = self.video_load.begin(requests);
        self.video_request = Some(request);
        request
    }
    pub fn attach_video(&mut self, task: tokio::task::AbortHandle) {
        self.video_task = Some(task);
    }
    pub fn complete_video(
        &mut self,
        source: &SourceId,
        request: LibraryRequest,
        result: Result<VideoTracks, String>,
    ) -> bool {
        if self.source.as_ref() != Some(source) {
            return false;
        }
        let result = result.and_then(|video| {
            if video.source != *source
                || video.songs.is_empty()
                || video.selected >= video.songs.len()
                || video
                    .songs
                    .iter()
                    .any(|song| song.source.as_ref() != Some(source))
            {
                Err("invalid video snapshot".into())
            } else {
                Ok(video)
            }
        });
        let Some(result) = self.video_load.complete(request, result) else {
            return false;
        };
        self.video_task = None;
        if let Ok(video) = result {
            self.video = Some(video);
        }
        true
    }
    pub fn playback_plan(
        &self,
        source: &SourceId,
        request: LibraryRequest,
        index: usize,
    ) -> Option<(Vec<crate::models::Song>, usize)> {
        if self.source.as_ref() != Some(source) || self.video_request != Some(request) {
            return None;
        }
        let video = self.video.as_ref()?;
        if video.source != *source || index >= video.songs.len() {
            return None;
        }
        Some((video.songs.clone(), index))
    }

    pub fn close_video(&mut self) {
        if let Some(task) = self.video_task.take() {
            task.abort();
        }
        self.video = None;
        self.video_load = Default::default();
        self.video_request = None;
        self.video_input.clear();
    }
    pub fn reject_input(&mut self, message: String) {
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        self.search_load = Default::default();
        self.results = None;
        self.close_video();
        self.input_error = Some(message);
    }
    pub fn search_loading(&self) -> bool {
        self.search_load.loading()
    }
    pub fn search_error(&self) -> Option<&str> {
        self.search_load.error()
    }
    pub fn video_loading(&self) -> bool {
        self.video_load.loading()
    }
    pub fn video_error(&self) -> Option<&str> {
        self.video_load.error()
    }
    pub fn preview_visible(&self) -> bool {
        self.video.is_some() || self.video_loading() || self.video_error().is_some()
    }
}
#[cfg(test)]
mod tests;
