//! 服务器管理、设置持久化以及音频和外观偏好变更。

use super::*;

impl NavidromeApp {
    /// 加载当前服务器作用域下的响度缓存、流派缓存与播放会话。
    /// 启动与切换服务器时都走这里；切换时调用方已清空 `state`。
    pub(super) fn load_active_server_cache(&mut self) {
        let server_id = self.config.active_server_id().map(str::to_string);
        self.state.loudness_cache = config::load_loudness(server_id.as_deref());
        self.state.listening_history = config::load_listening_history(server_id.as_deref());
        self.state
            .playback_history
            .restore(&self.state.listening_history);
        let history_covers = self
            .state
            .playback_history
            .songs()
            .iter()
            .filter_map(|song| song.cover_art.clone())
            .take(40)
            .collect();
        self.preload_covers(history_covers);
        let day = crate::recommendation::current_day_key();
        let cached_daily_mix = config::load_daily_mix(server_id.as_deref());
        self.state.daily_mix_previous_ids = cached_daily_mix
            .iter()
            .flat_map(|snapshot| snapshot.tracks.iter())
            .map(|track| track.song.id.clone())
            .collect();
        self.state.daily_mix = cached_daily_mix
            .filter(|snapshot| snapshot.is_current(server_id.as_deref().unwrap_or_default(), &day));
        let daily_accent_cover = self
            .state
            .daily_mix
            .iter()
            .flat_map(|snapshot| snapshot.tracks.iter())
            .find_map(|track| track.song.cover_art.clone());
        let daily_covers = self
            .state
            .daily_mix
            .iter()
            .flat_map(|snapshot| snapshot.tracks.iter())
            .filter_map(|track| track.song.cover_art.clone())
            .take(30)
            .collect();
        self.ensure_cover(daily_accent_cover.as_deref(), true);
        self.preload_covers(daily_covers);
        // 流派是服务端重查询（全库聚合）：先用磁盘缓存即时展示，后台刷新后覆盖。
        self.state.genres = config::load_genres_cache(server_id.as_deref()).unwrap_or_default();

        // 恢复该服务器的播放会话（不自动播放，点播放时从保存位置继续）。
        let Some(session) =
            config::load_session(server_id.as_deref(), self.config.server_url.trim())
        else {
            return;
        };
        if session.queue.is_empty() {
            return;
        }
        // 会话里记录的地址必须与当前服务器一致，避免拿错队列。
        let server_matches = self
            .api
            .as_ref()
            .map(|api| api.base_url() == session.server_url)
            .unwrap_or(false);
        if !server_matches {
            return;
        }
        let index = session
            .queue_index
            .filter(|index| *index < session.queue.len());
        let restored_index = index;
        self.state
            .replace_queue_with_origins(session.queue, session.queue_origins);
        self.state.queue_index = index;
        self.state.playback_modes = session.playback_modes;
        self.config.playback_modes = session.playback_modes;
        self.state.shuffle_history =
            ShuffleHistory::restore(session.shuffle_played, session.shuffle_forward);
        if let Some(index) = restored_index {
            if let Some(song) = self.state.queue.get(index).cloned() {
                self.state.now_playing = Some(song.clone());
                self.resume_position =
                    Some(Duration::from_secs_f64(session.position_secs.max(0.0)));
                self.ensure_cover(song.cover_art.as_deref(), true);
            }
        }
        log::info!(
            "restored playback session; server={:?} queue={} index={restored_index:?} position={:.1}s",
            self.config.active_server_id(),
            self.state.queue.len(),
            session.position_secs
        );
    }

    /// 把设置页的四个输入框同步为当前服务器（切换后必须调用，否则
    /// 用户看到并保存的是上一台服务器的账号）。密码同样解析后展示，
    /// 已迁入凭据库的条目回显真实密码，用户改密码时无需重新输入。
    pub(super) fn sync_server_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.config.active_profile().cloned() else {
            return;
        };
        self.fill_server_inputs(&profile, window, cx);
    }

    /// 断开当前服务器并重建连接与视图状态（切服务器、保存设置共用）。
    pub(super) fn reconnect_active_server(&mut self) {
        self.cancel_recommendation_tasks();
        self.daily_mix_generation = self.daily_mix_generation.wrapping_add(1);
        self.cancel_next_preload();
        self.finish_listening_event(false);
        self.audio.stop();
        self.pending_cover_asset_releases
            .borrow_mut()
            .extend(self.state.covers.values().cloned());
        self.cover_image_lru.borrow_mut().clear();
        self.pending_cover_image_evictions.borrow_mut().clear();
        self.state = AppState::default();
        self.playlist_ui.reset();
        self.import_ui.reset();
        self.resume_position = None;
        self.reconnect = None;
        self.reconnect_attempts = None;
        // 上一台服务器的曲目不应继续出现在“正在播放”与通知里。
        self.last_notified_track = None;
        self.state.playback_modes = self.config.playback_modes;
        self.state.albums_sort = self.config.albums_sort;
        self.state.artists_sort = self.config.artists_sort;
        // 空白条目（刚新增还没填）不是错误：只清空连接，等用户保存地址。
        let configured = self
            .config
            .active_profile()
            .is_some_and(|profile| profile.is_configured());
        if !configured {
            self.api = None;
            self.state.status = t!("Enter the server address and account").to_string();
            self.state.settings_open = true;
            return;
        }
        // 已迁入系统凭据库的条目用解析后的密码连接。
        let password = self
            .config
            .active_profile()
            .map(|profile| profile.resolved_password())
            .unwrap_or_default();
        match OpenSubsonicProvider::new(&self.config.server_url, &self.config.username, &password) {
            Ok(api) => {
                self.api = Some(api);
                self.load_active_server_cache();
                self.refresh_library();
            }
            Err(error) => {
                self.api = None;
                self.state.status = t!("Invalid server URL").to_string();
                self.notify_error(format!("{error:#}"));
                self.state.settings_open = true;
            }
        }
    }

    /// 切换到另一台已保存的服务器。切换前先把当前服务器的作用域数据落盘。
    pub(super) fn switch_server(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.config.active_server.as_deref() == Some(id) {
            return;
        }
        if !self.config.servers.iter().any(|profile| profile.id == id) {
            return;
        }
        // 队列属于当前服务器：先保存，再断开。
        self.save_session();
        if let Err(error) =
            config::save_loudness(self.config.active_server_id(), &self.state.loudness_cache)
        {
            log::warn!("failed to save loudness cache: {error:#}");
        }
        if !self.config.switch_to(id) {
            return;
        }
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        log::info!(
            "switched server; id={id} url={} user={}",
            self.config.server_url,
            self.config.username
        );
        self.state.status =
            t!("Switched to %{server}", server = self.active_server_label()).to_string();
        self.reconnect_active_server();
        self.sync_server_inputs(window, cx);
    }

    /// 新增一台空白服务器并立即切过去，方便直接填地址账号。
    pub(super) fn add_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_session();
        // 切走前把当前服务器的作用域数据落盘（与 switch_server 同等义务）。
        if let Err(error) =
            config::save_loudness(self.config.active_server_id(), &self.state.loudness_cache)
        {
            log::warn!("failed to save loudness cache: {error:#}");
        }
        let id = self.config.add_server();
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        log::info!("added server entry; id={id}");
        self.reconnect_active_server();
        self.sync_server_inputs(window, cx);
        // 新条目直接展开表单：新增的意图就是填写连接信息。
        self.state.editing_server = Some(id);
        self.state.status = t!("Enter the address and account for the new server").to_string();
    }

    /// 删除一台服务器；正在使用的条目被删除时自动切到相邻条目。
    /// clean_data = true 时一并清理这台服务器的作用域缓存（播放会话/
    /// 响度/流派）与系统凭据库条目；凭据删除不可恢复，重新添加同一台
    /// 服务器需要重输密码，因此默认保留。
    pub(super) fn remove_server(
        &mut self,
        id: &str,
        clean_data: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let removing_active = self.config.active_server.as_deref() == Some(id);
        let Some(profile) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return;
        };
        if removing_active {
            self.save_session();
            // 与切换服务器同等的落盘义务：响度缓存也属于当前服务器。
            if let Err(error) =
                config::save_loudness(self.config.active_server_id(), &self.state.loudness_cache)
            {
                log::warn!("failed to save loudness cache: {error:#}");
            }
        }
        if !self.config.remove_server(id) {
            self.notify_error(t!("Keep at least one server configured"));
            return;
        }
        if clean_data {
            if profile.password_in_keyring {
                secrets::delete_password(id);
            }
            let removed = config::remove_server_cache(id);
            log::info!("removed {removed} scoped cache file(s) of server {id}");
        }
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        log::info!("removed server entry; id={id}");
        // 正在编辑的条目被删掉：表单不能指向不存在的 id。
        if self.state.editing_server.as_deref() == Some(id) {
            self.state.editing_server = None;
        }
        if removing_active {
            self.reconnect_active_server();
            self.sync_server_inputs(window, cx);
        }
        self.state.status = t!("Removed %{server}", server = profile.label()).to_string();
    }

    /// 删除服务器前的确认弹窗（可选连同清理这台服务器的本地数据）。
    pub(super) fn confirm_remove_server(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(label) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.label())
        else {
            return;
        };
        if self.server_remove_prompt.is_some() {
            return;
        }
        self.server_remove_prompt = Some((id.to_string(), label));
        self.show_remove_server_dialog(window, cx);
    }

    /// 弹窗确认后的实际删除；clean_data 的语义与 remove_server 一致。
    /// 弹窗的关闭由调用方（按钮/框架）负责，这里只执行删除。
    pub(super) fn submit_remove_server(
        &mut self,
        clean_data: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((id, _)) = self.server_remove_prompt.take() else {
            return;
        };
        self.remove_server(&id, clean_data, window, cx);
        cx.notify();
    }

    /// 关闭弹窗（取消 / Esc / 点击遮罩）：只清状态，不删除。
    pub(super) fn cancel_remove_server(&mut self, cx: &mut Context<Self>) {
        self.server_remove_prompt = None;
        cx.notify();
    }

    pub(super) fn show_remove_server_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            // 与歌单弹窗同因：先等菜单层收尾，再安装弹窗的焦点域。
            window.defer(cx, move |window, cx| {
                window.open_dialog(cx, move |dialog, _, cx| {
                    app.update(cx, |this, cx| this.render_remove_server_dialog(dialog, cx))
                        .expect("app owns dialog")
                });
            });
        }
        cx.notify();
    }

    /// 当前服务器的显示名（侧边栏与设置页顶部共用）。
    pub(super) fn active_server_label(&self) -> String {
        self.config
            .active_profile()
            .map(|profile| profile.label())
            .unwrap_or_else(|| t!("No server").to_string())
    }

    pub(super) fn save_settings(&mut self, cx: &Context<Self>) {
        let url = self.server_input.read(cx).value().trim().to_string();
        let username = self.username_input.read(cx).value().trim().to_string();
        let password = self.password_input.read(cx).value().to_string();
        let display_name = self.name_input.read(cx).value().trim().to_string();
        // 表单目标：行内铅笔打开的条目；没有显式目标时（首次运行的
        // 表单直出场景）为激活条目。
        let target_id = self
            .state
            .editing_server
            .clone()
            .or_else(|| self.config.active_server.clone());
        let Some(target_id) = target_id else {
            return;
        };
        let Some(target_before) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == target_id)
            .cloned()
        else {
            return;
        };
        let is_active = self.config.active_server.as_deref() == Some(target_id.as_str());
        // 地址/账号/密码任一变化才需要断开重连；只改显示名或编辑的是
        // 另一台未连接的服务器时，正在播放的音乐不应被打断。输入框
        // 回填的是解析后的密码，因此未改动时两者相等。
        let identity_changed = target_before.url.trim() != url
            || target_before.username.trim() != username
            || target_before.resolved_password() != password;
        // 写回目标条目，而不是另外造一份配置：多服务器列表是唯一真源。
        // 激活条目走 store_active（处理与其它条目的身份合并）；非激活
        // 条目原位更新，id 与缓存命名空间保持不变。
        let stored_id = if is_active {
            let id = self.config.store_active(&url, &username, &password);
            if let Some(profile) = self
                .config
                .servers
                .iter_mut()
                .find(|profile| profile.id == id)
            {
                profile.name = display_name;
            }
            id
        } else if self
            .config
            .store_profile(&target_id, &url, &username, &password)
        {
            if let Some(profile) = self
                .config
                .servers
                .iter_mut()
                .find(|profile| profile.id == target_id)
            {
                profile.name = display_name;
            }
            target_id
        } else {
            return;
        };
        if let Some(profile) = self
            .config
            .servers
            .iter_mut()
            .find(|profile| profile.id == stored_id)
        {
            // 密码优先写入系统凭据库（里程碑 #17）：成功后配置里只留空
            // 引用，明文不再落盘；凭据库不可用时保留明文并落下标记。
            if !password.is_empty() {
                match secrets::store_password(&stored_id, &username, &password) {
                    Ok(()) => {
                        profile.password.clear();
                        profile.password_in_keyring = true;
                        log::info!(
                            "stored the password of server {stored_id} in the system credential store"
                        );
                    }
                    Err(error) => {
                        profile.password_in_keyring = false;
                        log::warn!(
                            "system credential store unavailable; keeping the password in the config file: {error:#}"
                        );
                    }
                }
            } else {
                // 用户主动清空密码：同步删掉凭据库条目，避免遗留旧密码。
                if profile.password_in_keyring {
                    secrets::delete_password(&stored_id);
                }
                profile.password_in_keyring = false;
            }
        }
        // 同步镜像字段（server_url/username/password 始终与激活条目一致）。
        // 直接在 self.config 上变更，不再重建整份 Config——手动罗列字段的
        // 重建在新字段加入时极易漏拷而静默丢失设置。
        self.config.sync_active_profile();
        if let Err(error) = config::save(&self.config) {
            self.state.status =
                t!("Config save failed: %{error}", error = format!("{error:#}")).to_string();
        }
        if is_active {
            // 未配置的条目（首次运行、刚新增还没填）不能走“仅保存”分支：
            // 保存后仍连不上，重连会给出正确的引导文案并保持表单展开。
            let still_unconfigured = self
                .config
                .active_profile()
                .is_none_or(|profile| !profile.is_configured());
            if identity_changed || still_unconfigured {
                self.reconnect_active_server();
                return;
            }
        }
        // 只改了显示名 / 编辑的是另一台未连接的服务器：保留当前连接与
        // 设置页，收起表单。“已保存”用 Toast：表单收起后状态行不在可见区。
        self.state.editing_server = None;
        self.notify_success(t!("Saved"));
    }

    /// 放弃编辑：输入框恢复为表单目标条目的已保存值并收起表单。
    /// 未配置的条目没有可恢复的内容，调用方负责不显示取消按钮。
    pub(super) fn cancel_server_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .state
            .editing_server
            .clone()
            .or_else(|| self.config.active_server.clone());
        if let Some(profile) = target
            .as_deref()
            .and_then(|id| self.config.servers.iter().find(|profile| profile.id == id))
            .cloned()
        {
            self.fill_server_inputs(&profile, window, cx);
        }
        self.state.editing_server = None;
        cx.notify();
    }

    /// 从指定条目填充表单的四个输入框（打开编辑与取消恢复共用）。
    pub(super) fn fill_server_inputs(
        &self,
        profile: &ServerProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = profile.name.clone();
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        let url = profile.url.clone();
        self.server_input
            .update(cx, |input, cx| input.set_value(url, window, cx));
        let username = profile.username.clone();
        self.username_input
            .update(cx, |input, cx| input.set_value(username, window, cx));
        let password = profile.resolved_password();
        self.password_input
            .update(cx, |input, cx| input.set_value(password, window, cx));
    }

    /// 打开指定条目的编辑表单。不切换服务器：修另一台的地址账号
    /// 不该把当前正在浏览/播放的库换掉。
    pub(super) fn begin_edit_server(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return;
        };
        self.state.editing_server = Some(id.to_string());
        self.fill_server_inputs(&profile, window, cx);
        cx.notify();
    }

    pub(super) fn apply_cache_directory(&mut self, cache_dir: Option<PathBuf>) {
        let effective_dir = cache_dir
            .clone()
            .unwrap_or_else(config::default_audio_cache_dir);
        if let Err(error) = fs::create_dir_all(&effective_dir) {
            self.notify_error(t!(
                "Unable to use cache directory %{dir}: %{error}",
                dir = effective_dir.display(),
                error = error
            ));
            return;
        }

        self.config.cache_dir = cache_dir;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Cache directory save failed: %{error}",
                error = format!("{error:#}")
            ));
            return;
        }
        self.cancel_next_preload();
        self.audio.set_cache_directory(effective_dir.clone());
        self.state.status = t!("Audio cache: %{dir}", dir = effective_dir.display()).to_string();
    }

    pub(super) fn choose_cache_directory(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("Select audio cache folder").into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .and_then(|paths| paths.into_iter().next());
            if let Some(path) = selected {
                this.update(cx, |this, cx| {
                    this.apply_cache_directory(Some(path));
                    cx.notify();
                })?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub(super) fn set_transcoding_quality(
        &mut self,
        quality: TranscodingQuality,
        cx: &mut Context<Self>,
    ) {
        self.cancel_next_preload();
        self.config.transcoding_quality = quality;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Playback quality save failed: %{error}",
                error = format!("{error:#}")
            ));
        } else {
            log::info!(
                "transcoding quality changed; quality={}",
                quality.cache_profile()
            );
        }
        cx.notify();
    }

    pub(super) fn set_volume_normalization(
        &mut self,
        mode: VolumeNormalization,
        cx: &mut Context<Self>,
    ) {
        self.cancel_next_preload();
        self.config.volume_normalization = mode;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Volume normalization save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    /// 歌词翻译行显示开关；服务器无多语言变体时该开关无效果。
    pub(super) fn set_show_lyrics_translation(&mut self, show: bool, cx: &mut Context<Self>) {
        self.config.show_lyrics_translation = show;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Lyrics translation save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    /// 枚举输出设备（后台线程，避免 cpal 枚举阻塞 UI）。
    pub(super) fn refresh_output_devices(&mut self, cx: &mut Context<Self>) {
        if self.state.output_devices_loading {
            return;
        }
        self.state.output_devices_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = crate::audio::list_output_devices().map_err(error_message);
            let _ = tx.send(Msg::OutputDevices(result));
        });
        cx.notify();
    }

    /// 单段均衡增益变化（滑杆拖动）：更新配置、推送音频线程、防抖保存。
    /// 任何拖动都会开启均衡器（与预设一致的心智模型）。
    pub(super) fn set_eq_band(&mut self, band: usize, gain_db: f32, cx: &mut Context<Self>) {
        let Some(current) = self.config.equalizer.gains_db.get_mut(band) else {
            return;
        };
        if (*current - gain_db).abs() < f32::EPSILON && self.config.equalizer.enabled {
            return;
        }
        *current = gain_db;
        self.config.equalizer.clamp_gains();
        self.config.equalizer.enabled = true;
        self.push_equalizer_to_audio();
        self.schedule_eq_save(cx);
        cx.notify();
    }

    /// 均衡器开关。
    pub(super) fn set_eq_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.config.equalizer.enabled == enabled {
            return;
        }
        self.config.equalizer.enabled = enabled;
        self.push_equalizer_to_audio();
        self.schedule_eq_save(cx);
        cx.notify();
    }

    /// 应用预设（不含滑杆显示同步——由调用方在可用的上下文里完成）：
    /// 更新配置、推送音频线程并开启均衡器。
    pub(super) fn set_eq_preset_values(&mut self, gains: [f32; 10], cx: &mut Context<Self>) {
        self.config.equalizer.gains_db = gains;
        self.config.equalizer.enabled = true;
        self.push_equalizer_to_audio();
        self.schedule_eq_save(cx);
        cx.notify();
    }

    pub(super) fn push_equalizer_to_audio(&self) {
        self.audio.set_equalizer(self.config.equalizer.clone());
    }

    /// 均衡器保存防抖（拖动滑杆时避免频繁写盘，500ms）。
    pub(super) fn schedule_eq_save(&mut self, cx: &mut Context<Self>) {
        self.eq_save_generation = self.eq_save_generation.wrapping_add(1);
        let generation = self.eq_save_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(500)).await;
            this.update(cx, |this, cx| {
                if this.eq_save_generation != generation {
                    return;
                }
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(t!(
                        "Equalizer save failed: %{error}",
                        error = format!("{error:#}")
                    ));
                }
                cx.notify();
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    /// 切换输出设备：持久化选择并通知音频线程；正在播放时
    /// 音频线程会在新设备上从当前位置继续，暂停意图保留。
    pub(super) fn set_output_device(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.config.output_device == name {
            return;
        }
        self.config.output_device = name.clone();
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Audio output save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.cancel_next_preload();
        self.audio.set_output_device(name);
        cx.notify();
    }

    /// 切换界面语言：写配置 + set_locale，下一次渲染整份 UI 文案生效。
    pub(super) fn set_language(&mut self, preference: LanguagePreference, cx: &mut Context<Self>) {
        if self.config.language == preference {
            return;
        }
        self.config.language = preference;
        let locale = preference.resolve();
        rust_i18n::set_locale(locale);
        crate::typography::configure_ui_font(cx, locale);
        // 托盘菜单是原生控件：重建一次让它也换成新语言。
        tray::refresh_labels();
        if let Err(error) = config::save(&self.config) {
            self.state.status =
                t!("Config save failed: %{error}", error = format!("{error:#}")).to_string();
        }
        cx.notify();
    }

    pub(super) fn set_theme(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.theme = preference;
        match preference {
            ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
            ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
        }
        if let Err(error) = config::save(&self.config) {
            self.state.status =
                t!("Theme save failed: %{error}", error = format!("{error:#}")).to_string();
        }
        cx.notify();
    }
}
