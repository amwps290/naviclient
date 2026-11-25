//! Explicit Bilibili search, preview and playback actions; never favorites writes.

use crate::app::prelude::*;
use crate::source::{
    bilibili::discovery::{parse_video_input, SearchPage, VideoTracks},
    SourceId,
};
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl NavidromeApp {
    pub(in crate::app) fn subscribe_bilibili_discovery(&mut self, cx: &mut Context<Self>) {
        let input = self.bilibili_search_input.clone();
        self._subscriptions
            .push(cx.subscribe(&input, |this, input, event, cx| {
                if !this.browsing_bilibili() || this.state.view != View::Search {
                    return;
                }
                match event {
                    InputEvent::Change => {
                        this.state.bilibili_discovery.draft = input.read(cx).value().to_string()
                    }
                    InputEvent::PressEnter { .. } => this.submit_bilibili_discovery(cx),
                    InputEvent::Focus | InputEvent::Blur => cx.notify(),
                }
            }));
    }

    pub(in crate::app) fn submit_bilibili_discovery(&mut self, cx: &mut Context<Self>) {
        let value = self
            .bilibili_search_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if value.is_empty() {
            self.state
                .bilibili_discovery
                .reject_input(t!("Enter keywords, a BV/AV ID, or a video link.").to_string());
            cx.notify();
            return;
        }
        match parse_video_input(&value) {
            Ok(Some(_)) => self.open_bilibili_discovery_video(value, cx),
            Ok(None) => self.search_bilibili_page(value, 1, cx),
            Err(error) => {
                self.state
                    .bilibili_discovery
                    .reject_input(error.to_string());
                cx.notify();
            }
        }
    }

    pub(in crate::app) fn search_bilibili_page(
        &mut self,
        keyword: String,
        page: u32,
        cx: &mut Context<Self>,
    ) {
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.id().provider == crate::source::ProviderKind::Bilibili)
        else {
            return;
        };
        let source = api.id().clone();
        let request = self.state.bilibili_discovery.begin_search(
            &source,
            keyword.clone(),
            page,
            &mut self.library_requests,
        );
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result =
                tokio::time::timeout(REQUEST_TIMEOUT, api.search_bilibili_videos(&keyword, page))
                    .await
                    .map_err(|_| t!("Bilibili search timed out. Retry.").to_string())
                    .and_then(|result| result.map_err(error_message));
            let _ = tx.send(Msg::BilibiliSearch {
                source,
                request,
                result,
            });
        });
        self.state
            .bilibili_discovery
            .attach_search(task.abort_handle());
        cx.notify();
    }

    pub(in crate::app) fn open_bilibili_discovery_video(
        &mut self,
        input: String,
        cx: &mut Context<Self>,
    ) {
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.id().provider == crate::source::ProviderKind::Bilibili)
        else {
            return;
        };
        let source = api.id().clone();
        self.bilibili_parts_scroll = UniformListScrollHandle::new();
        let request = self.state.bilibili_discovery.begin_video(
            &source,
            input.clone(),
            &mut self.library_requests,
        );
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result = tokio::time::timeout(REQUEST_TIMEOUT, api.open_bilibili_video(&input))
                .await
                .map_err(|_| t!("Bilibili video lookup timed out. Retry.").to_string())
                .and_then(|result| result.map_err(error_message));
            let _ = tx.send(Msg::BilibiliVideo {
                source,
                request,
                result,
            });
        });
        self.state
            .bilibili_discovery
            .attach_video(task.abort_handle());
        cx.notify();
    }

    pub(in crate::app) fn finish_bilibili_search(
        &mut self,
        source: SourceId,
        request: LibraryRequest,
        result: Result<SearchPage, String>,
    ) {
        if self.api.as_ref().map(|api| api.id()) != Some(&source)
            || !self
                .state
                .bilibili_discovery
                .complete_search(&source, request, result)
        {
            return;
        }
        let covers = self
            .state
            .bilibili_discovery
            .results
            .iter()
            .flat_map(|page| &page.videos)
            .filter_map(|video| video.cover.clone())
            .collect();
        self.preload_covers(covers);
    }
    pub(in crate::app) fn finish_bilibili_video(
        &mut self,
        source: SourceId,
        request: LibraryRequest,
        result: Result<VideoTracks, String>,
    ) {
        if self.api.as_ref().map(|api| api.id()) != Some(&source)
            || !self
                .state
                .bilibili_discovery
                .complete_video(&source, request, result)
        {
            return;
        }
        let cover = self
            .state
            .bilibili_discovery
            .video
            .as_ref()
            .and_then(|video| video.cover.clone());
        self.ensure_cover(cover.as_deref(), false);
        if let Some(video) = self.state.bilibili_discovery.video.as_ref() {
            self.bilibili_parts_scroll
                .scroll_to_item(video.selected, gpui_kit::ScrollStrategy::Nearest);
        }
    }

    pub(in crate::app) fn select_bilibili_preview_part(
        &mut self,
        request: LibraryRequest,
        index: usize,
    ) -> bool {
        if self.state.bilibili_discovery.video_request != Some(request) {
            return false;
        }
        let Some(video) = self.state.bilibili_discovery.video.as_mut() else {
            return false;
        };
        if self.api.as_ref().map(|api| api.id()) != Some(&video.source)
            || index >= video.songs.len()
        {
            return false;
        }
        video.selected = index;
        true
    }

    pub(in crate::app) fn play_bilibili_preview(
        &mut self,
        request: LibraryRequest,
        index: usize,
        append: bool,
    ) {
        let Some(source) = self.api.as_ref().map(|api| api.id()) else {
            return;
        };
        let Some((songs, index)) = self
            .state
            .bilibili_discovery
            .playback_plan(source, request, index)
        else {
            return;
        };
        if append {
            self.append_all_with_origin(songs, QueueOrigin::Manual);
        } else {
            self.replace_queue_and_play(songs, index, QueueOrigin::Manual);
        }
    }
}
