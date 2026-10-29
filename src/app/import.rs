//! 歌单导入向导：粘贴链接 → 拉取 → 匹配 → 审查 → 在 Navidrome 创建歌单。
//!
//! 三个阶段共用一个对话框（`ImportStage`）：
//! - `Url`：粘贴分享文案，回车或“获取”开始；
//! - `Matching`：拉取歌单 + 逐首匹配曲库，进度条实时更新；
//! - `Review`：展示 ✓/✗ 明细与预填的歌单名，确认后走 `PlaylistEdit::Create`。
//!
//! generation 每次“开始/取消/切服务器”自增，让在途的后台结果作废——
//! 与 `PlaylistUi` 的 generation 机制同一套防御。

use super::*;
use crate::import;
use crate::playlist::PlaylistEdit;
use gpui_kit::component::dialog::Dialog;

/// 导入向导的持久状态。字段全部 Option/空值起步，Default 即“未打开”。
#[derive(Default)]
pub(super) struct ImportUi {
    pub generation: u64,
    pub stage: Option<ImportStage>,
    /// `Url` 阶段的链接输入框。
    pub url_input: Option<Entity<InputState>>,
    /// `Review` 阶段的歌单名输入框（预填外部歌单名）。
    pub name_input: Option<Entity<InputState>>,
    pub playlist: Option<import::ImportedPlaylist>,
    /// 与 `playlist.tracks` 一一对应的匹配结果；None = 未匹配。
    pub matches: Vec<Option<Song>>,
    /// 对应曲目是否因服务器请求错误而未完成检查。
    pub match_failures: Vec<bool>,
    pub progress: (usize, usize),
    pub error: Option<String>,
    pub subscription: Option<Subscription>,
    /// 当前解析或匹配任务；reset 时主动 abort，避免取消后继续请求服务器。
    pub task: Option<tokio::task::AbortHandle>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImportStage {
    /// 粘贴链接。
    Url,
    /// 解析 / 拉取 / 匹配进行中（对话框不可误关，但可以显式取消）。
    Matching,
    /// 匹配完成，审查并确认。
    Review,
}

impl ImportUi {
    /// 全量复位（关闭、取消、切服务器、导入完成）。先终止后台任务，再自增
    /// generation，让已经进入消息队列的旧结果也因代次不符被丢弃。
    pub fn reset(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        *self = Self {
            generation: self.generation.wrapping_add(1),
            ..Self::default()
        };
    }
}

impl NavidromeApp {
    /// 导入流程是否占用对话框：匹配进行中，或正在保存创建的歌单。
    pub(super) fn import_busy(&self) -> bool {
        self.import_ui.stage == Some(ImportStage::Matching) || self.playlist_ui.busy
    }

    pub(super) fn open_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.api.is_none()
            || self.playlist_ui.busy
            || self.playlist_ui.dialog_open()
            || self.import_ui.stage.is_some()
        {
            return;
        }
        self.import_ui.reset();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(t!("Playlist link")));
        self.import_ui.subscription =
            Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_import_step(cx);
                }
            }));
        self.import_ui.url_input = Some(input.clone());
        self.import_ui.stage = Some(ImportStage::Url);
        self.import_ui.error = None;
        self.show_import_dialog(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn show_import_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            // 与 show_playlist_dialog 相同：让菜单先收起再装焦点域，
            // 否则菜单收起时的焦点恢复会吞掉回车/Escape。
            window.defer(cx, move |window, cx| {
                let render_app = app.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| this.render_import_dialog(dialog, cx))
                        .expect("app owns dialog")
                });
                let _ = app.update(cx, |this, cx| {
                    if this.import_ui.stage == Some(ImportStage::Url) {
                        if let Some(input) = this.import_ui.url_input.clone() {
                            input.update(cx, |input, cx| input.focus(window, cx));
                        }
                    }
                });
            });
        }
        cx.notify();
    }

    /// 对话框主按钮 / 回车：按阶段分发。
    pub(super) fn submit_import_step(&mut self, cx: &mut Context<Self>) {
        match self.import_ui.stage {
            Some(ImportStage::Url) => self.submit_import_url(cx),
            Some(ImportStage::Review) => self.submit_import_playlist(cx),
            Some(ImportStage::Matching) | None => {}
        }
    }

    /// `Url` 阶段提交：解析链接 + 拉取歌单元数据（第一阶段）。
    fn submit_import_url(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.import_ui.url_input.clone() else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.import_ui.error = Some(t!("Unrecognized playlist link.").to_string());
            cx.notify();
            return;
        }
        self.import_ui.generation = self.import_ui.generation.wrapping_add(1);
        let generation = self.import_ui.generation;
        self.import_ui.stage = Some(ImportStage::Matching);
        self.import_ui.error = None;
        self.import_ui.progress = (0, 0);
        let http = import::http_client();
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result = async {
                let source = import::resolve(&text, &http).await?;
                source.fetch(&http).await
            }
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::ImportResolved { generation, result });
        });
        self.import_ui.task = Some(task.abort_handle());
        cx.notify();
    }

    /// 第一阶段结果落地：成功则立刻进入匹配，失败退回链接输入。
    pub(super) fn finish_import_resolve(
        &mut self,
        generation: u64,
        result: Result<import::ImportedPlaylist, String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.import_ui.generation {
            return; // 已取消或被新一轮导入取代。
        }
        self.import_ui.task = None;
        match result {
            Ok(playlist) => {
                // 匹配需要曲库搜索；极小概率下导入中途断连，退回链接阶段
                // 让用户重试，不要把对话框卡在“匹配中”。
                let Some(api) = self.api.clone() else {
                    self.import_ui.stage = Some(ImportStage::Url);
                    self.import_ui.error = Some(t!("Not connected").to_string());
                    cx.notify();
                    return;
                };
                let total = playlist.tracks.len();
                self.import_ui.playlist = Some(playlist.clone());
                self.import_ui.progress = (0, total);
                let tracks = playlist.tracks;
                let tx = self.tx.clone();
                let task = self.runtime.spawn(async move {
                    let progress = {
                        let tx = tx.clone();
                        move |done: usize| {
                            let _ = tx.send(Msg::ImportProgress { generation, done });
                        }
                    };
                    let result = import::match_playlist(&api, &tracks, progress)
                        .await
                        .map_err(error_message);
                    let _ = tx.send(Msg::ImportMatched { generation, result });
                });
                self.import_ui.task = Some(task.abort_handle());
            }
            Err(error) => {
                self.import_ui.stage = Some(ImportStage::Url);
                self.import_ui.error = Some(error);
            }
        }
        cx.notify();
    }

    /// 匹配进度：只更新计数，交给 UI 循环统一重绘。
    pub(super) fn finish_import_progress(&mut self, generation: u64, done: usize) {
        if generation == self.import_ui.generation {
            if let Some(total) = self.import_ui.playlist.as_ref().map(|p| p.tracks.len()) {
                self.import_ui.progress = (done.min(total), total);
            }
        }
    }

    /// 第二阶段结果落地：成功进入审查，失败退回链接输入。
    pub(super) fn finish_import_match(
        &mut self,
        generation: u64,
        result: Result<import::MatchedSongs, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if generation != self.import_ui.generation {
            return;
        }
        self.import_ui.task = None;
        match result {
            Ok(result) => {
                let matched = result.songs.iter().filter(|song| song.is_some()).count();
                let failures = result.failures.iter().filter(|failed| **failed).count();
                self.import_ui.matches = result.songs;
                self.import_ui.match_failures = result.failures;
                if matched == 0 {
                    self.import_ui.error = Some(if failures > 0 {
                        t!("No tracks are ready to import. Cancel and try again.").to_string()
                    } else {
                        t!("No tracks matched your library.").to_string()
                    });
                }
                let name = self
                    .import_ui
                    .playlist
                    .as_ref()
                    .map(|playlist| playlist.name.clone())
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(t!("Playlist name"))
                        .default_value(name)
                });
                self.import_ui.subscription =
                    Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::PressEnter { .. }) {
                            this.submit_import_step(cx);
                        }
                    }));
                self.import_ui.name_input = Some(input.clone());
                self.import_ui.stage = Some(ImportStage::Review);
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            Err(error) => {
                self.import_ui.stage = Some(ImportStage::Url);
                self.import_ui.error = Some(error);
            }
        }
        cx.notify();
    }

    /// `Review` 阶段确认：把匹配到的歌曲按原顺序建成 Navidrome 歌单。
    fn submit_import_playlist(&mut self, cx: &mut Context<Self>) {
        if self.playlist_ui.busy {
            return;
        }
        let Some(input) = self.import_ui.name_input.clone() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.import_ui.error = Some(t!("Enter a playlist name.").to_string());
            cx.notify();
            return;
        }
        let songs: Vec<String> = self
            .import_ui
            .matches
            .iter()
            .filter_map(|song| song.as_ref().map(|song| song.id.clone()))
            .collect();
        if songs.is_empty() {
            self.import_ui.error = Some(t!("No tracks matched your library.").to_string());
            cx.notify();
            return;
        }
        // 保存走共享的 playlist_ui.busy/generation 通道；完成消息由
        // finish_playlist_edit 关闭对话框（见其 Ok 分支对 import_ui 的处理）。
        self.start_playlist_edit(PlaylistEdit::Create { name, songs }, cx);
    }

    /// 显式取消（页脚按钮）：立即复位并关窗，在途结果靠 generation 作废。
    fn close_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // createPlaylist 已经发出后无法可靠撤销；保存完成前不允许用页脚绕过
        // dialog 的 busy 关闭保护。
        if self.playlist_ui.busy {
            return;
        }
        self.import_ui.reset();
        window.close_dialog(cx);
        cx.notify();
    }

    fn render_import_dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let busy = self.import_busy();
        let matched_count = self
            .import_ui
            .matches
            .iter()
            .filter(|song| song.is_some())
            .count();
        let failure_count = self
            .import_ui
            .match_failures
            .iter()
            .filter(|failed| **failed)
            .count();
        let mut content = v_flex().gap_3();
        let action: Option<String> = match self.import_ui.stage {
            Some(ImportStage::Url) => {
                if let Some(input) = &self.import_ui.url_input {
                    content = content
                        .child(
                            div()
                                .text_sm()
                                .child(t!(
                                    "Paste a NetEase Cloud Music or QQ Music playlist link."
                                )),
                        )
                        .child(Input::new(input).w_full())
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!(
                                "Direct links, mobile share links, short links, and full share messages are supported."
                            )),
                        );
                }
                Some(t!("Fetch").to_string())
            }
            Some(ImportStage::Matching) => {
                let (done, total) = self.import_ui.progress;
                let ratio = if total > 0 {
                    done as f64 / total as f64
                } else {
                    0.0
                };
                let label = if total > 0 {
                    t!(
                        "Matching tracks… %{done}/%{total}",
                        done = done,
                        total = total
                    )
                } else {
                    t!("Fetching playlist…")
                };
                let bar = readable_accent(cx.theme().accent, cx.theme().background);
                if let Some(playlist) = self.import_ui.playlist.as_ref() {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(playlist.name.clone()),
                    );
                }
                content = content.child(div().text_sm().child(label)).child(
                    div()
                        .h(px(6.0))
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().border)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .w(relative(ratio as f32))
                                .bg(bar),
                        ),
                );
                None
            }
            Some(ImportStage::Review) => {
                let total = self
                    .import_ui
                    .playlist
                    .as_ref()
                    .map(|playlist| playlist.tracks.len())
                    .unwrap_or(0);
                let unmatched_count = total.saturating_sub(matched_count + failure_count);
                let unavailable_count = self
                    .import_ui
                    .playlist
                    .as_ref()
                    .map_or(0, |playlist| playlist.unavailable_tracks);
                if let Some(input) = &self.import_ui.name_input {
                    content = content
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("Playlist name")),
                        )
                        .child(Input::new(input).w_full().disabled(self.playlist_ui.busy));
                }
                content = content
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(cx.theme().success.opacity(0.12))
                                    .text_xs()
                                    .text_color(cx.theme().success)
                                    .child(t!("Matched %{count}", count = matched_count)),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(cx.theme().secondary)
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No match %{count}", count = unmatched_count)),
                            )
                            .when(failure_count > 0, |summary| {
                                summary.child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(cx.theme().danger.opacity(0.1))
                                        .text_xs()
                                        .text_color(cx.theme().danger)
                                        .child(t!("Check failed %{count}", count = failure_count)),
                                )
                            })
                            .when(unavailable_count > 0, |summary| {
                                summary.child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(cx.theme().info.opacity(0.1))
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
                                        .child(t!(
                                            "Unavailable %{count}",
                                            count = unavailable_count
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("Only matched tracks will be imported.")),
                    );
                if failure_count > 0 {
                    content = content.child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(cx.theme().danger.opacity(0.08))
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(t!(
                                "Some tracks could not be checked because the server request failed. You can import the rest or cancel and try again."
                            )),
                    );
                }
                if unavailable_count > 0 {
                    content = content.child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(cx.theme().info.opacity(0.1))
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(t!(
                                "%{count} tracks are unavailable from the source service and were skipped before matching.",
                                count = unavailable_count
                            )),
                    );
                }
                let rows: Vec<_> = self
                    .import_ui
                    .playlist
                    .iter()
                    .flat_map(|playlist| playlist.tracks.iter().zip(self.import_ui.matches.iter()))
                    .enumerate()
                    .map(|(index, (track, matched))| {
                        let failed = self
                            .import_ui
                            .match_failures
                            .get(index)
                            .copied()
                            .unwrap_or(false);
                        let (status, color, background) = match (matched, failed) {
                            (Some(_), _) => (
                                t!("Matched").to_string(),
                                cx.theme().success,
                                cx.theme().success.opacity(0.12),
                            ),
                            (None, true) => (
                                t!("Check failed").to_string(),
                                cx.theme().danger,
                                cx.theme().danger.opacity(0.1),
                            ),
                            (None, false) => (
                                t!("No match").to_string(),
                                cx.theme().muted_foreground,
                                cx.theme().secondary,
                            ),
                        };
                        let source = format!("{} · {}", track.title, track.artists.join(" / "));
                        v_flex()
                            .gap_1()
                            .p_3()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.7))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div().flex_1().min_w_0().text_sm().truncate().child(source),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(background)
                                            .text_xs()
                                            .text_color(color)
                                            .child(status),
                                    ),
                            )
                            .when_some(matched.as_ref(), |row, song| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(format!("→ {} · {}", song.title, song.artist)),
                                )
                            })
                    })
                    .collect();
                content = content.child(
                    v_flex()
                        .id("import-review-list")
                        .max_h(px(260.0))
                        .overflow_y_scroll()
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded_lg()
                        .children(rows),
                );
                Some(t!("Import %{count} tracks", count = matched_count).to_string())
            }
            None => return dialog,
        };
        if let Some(error) = &self.import_ui.error {
            content = content.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(cx.theme().danger.opacity(0.08))
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        if self.playlist_ui.busy {
            content = content.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(cx.theme().info.opacity(0.1))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(t!("Saving…")),
            );
        }
        // Review 阶段没有任何匹配项时主按钮禁用（错误行会说明原因）。
        let review_empty = self.import_ui.stage == Some(ImportStage::Review) && matched_count == 0;
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-import-dialog")
                    .label(t!("Cancel"))
                    .outline()
                    .disabled(self.playlist_ui.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_import_dialog(window, cx);
                    })),
            )
            .when_some(action, |footer, action| {
                footer.child(
                    Button::new("submit-import-dialog")
                        .label(action)
                        .primary()
                        .disabled(busy || review_empty)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_import_step(cx))),
                )
            });
        let submit = cx.entity().downgrade();
        let cancel = submit.clone();
        dialog
            .title(t!("Import playlist").to_string())
            .w(px(560.0))
            .overlay_closable(false)
            .close_button(!busy)
            .keyboard(!busy)
            .on_ok(move |_, _, cx| {
                let _ = submit.update(cx, |this, cx| this.submit_import_step(cx));
                false
            })
            .on_cancel(move |_, _, cx| {
                cancel
                    .update(cx, |this, _| !this.import_busy())
                    .unwrap_or(true)
            })
            .on_close(cx.listener(|this, _, _, cx| {
                this.import_ui.reset();
                cx.notify();
            }))
            .child(content)
            .footer(footer)
    }
}
