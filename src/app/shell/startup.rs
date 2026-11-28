//! 应用构造、控件订阅、桌面集成初始化。

use crate::app::prelude::*;

impl Drop for NavidromeApp {
    fn drop(&mut self) {
        self.state.settings.bilibili_login.cancel();
        // Closing the receiver rejects even store completions arriving during
        // shutdown, allowing their staging guards to roll back their entries.
        let (_, closed_rx) = mpsc::channel();
        drop(std::mem::replace(&mut self.rx, closed_rx));
        self.save_playback_before_exit();
        // Close the bounded frame channel before Runtime::drop waits for its
        // blocking worker; otherwise a producer could wait on a full queue.
        self.vinyl_cover.cancel_pending();
        let pending_favorites = self.favorite_writes.clone();
        let _ = self.runtime.block_on(async {
            // Source methods bound preflight, POST and verification to <=70s.
            // Keep the runtime alive for accepted writes during graceful exit.
            let _ = tokio::time::timeout(Duration::from_secs(90), async {
                while pending_favorites.load(Ordering::SeqCst) > 0 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
            .await;
            tokio::time::timeout(
                Duration::from_secs(30),
                secrets::source_credentials::settle_staging(),
            )
            .await
        });
    }
}

impl NavidromeApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut config = config::load();
        // 界面语言：System 跟随操作系统。渲染处每次 t! 都查询当前
        // locale，因此运行时 set_locale 即时生效，无需重启。
        rust_i18n::set_locale(config.language.resolve());
        // 一次性迁移（里程碑 #17）：把旧版明文密码移入系统凭据库，
        // 迁移成功的条目在配置里只留空引用；凭据库不可用时保持明文，
        // 每轮启动重试，绝不丢密码。
        let migrated = secrets::migrate_config(&mut config);
        if !migrated.is_empty() {
            match config::save(&config) {
                Ok(()) => log::info!(
                    "migrated {} server password(s) into the system credential store",
                    migrated.len()
                ),
                Err(error) => {
                    log::warn!("failed to persist the credential migration: {error:#}")
                }
            }
        }
        // 连接使用解析后的密码：已迁入凭据库的条目从凭据库读取。
        let active_password = config
            .active_profile()
            .map(|profile| profile.resolved_password())
            .unwrap_or_default();
        let api = config.active_source.as_ref().and_then(|source| {
            crate::app::playback::source_context::configured_source(&config, source)
        });
        let playback_source = config
            .playback_source
            .as_ref()
            .and_then(|source| {
                crate::app::playback::source_context::configured_source(&config, source)
            })
            .or_else(|| {
                config
                    .playback_source
                    .is_none()
                    .then(|| api.clone())
                    .flatten()
            });
        if let Some(api) = &playback_source {
            config.playback_source = Some(api.id().clone());
        }
        let (tx, rx) = mpsc::channel();
        let worker_threads = std::thread::available_parallelism()
            .map(|count| count.get().clamp(2, 4))
            .unwrap_or(2);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .thread_name("navidrome-tokio")
            .enable_all()
            .build()
            .expect("failed to create Tokio runtime");
        let media_session = MediaSession::new(runtime.handle(), main_window_hwnd(window));
        let global_shortcuts = GlobalShortcutManager::new(&config.global_shortcuts);
        let requested_cache_dir = config::audio_cache_dir(&config);
        let cache_directory_error = if config.cache_dir.is_some() {
            config::prepare_audio_cache_dir(&config).err()
        } else {
            None
        };
        if let Some(error) = &cache_directory_error {
            log::warn!("using the default audio cache directory: {error}");
            config.cache_dir = None;
        }
        let audio_cache_dir = config::audio_cache_dir(&config);
        let offline_dir = config::offline_audio_dir(&config);
        let offline_store = crate::offline::OfflineStore::open(offline_dir.clone());
        let offline_outbox = crate::offline::OfflineOutbox::open(&offline_dir);
        let audio = AudioHandle::start(audio_cache_dir, config.output_device.clone())
            .expect("failed to start audio worker");
        audio.set_equalizer(config.equalizer.clone());
        let default_cover = Arc::new(GpuiImage::from_bytes(
            GpuiImageFormat::Png,
            include_bytes!("../../../assets/default-cover.png").to_vec(),
        ));
        let cover_textures = CoverTextures::new(
            default_cover.id(),
            crate::app::covers::textures::MAX_DECODED_COVERS,
        );
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Search songs, albums, and artists"))
                .clean_on_escape()
        });
        let server_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("http://localhost:4533"))
                .default_value(config.server_url.clone())
        });
        let username_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Username"))
                .default_value(config.username.clone())
        });
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Password"))
                .default_value(active_password)
                .masked(true)
        });
        // 服务器显示名（可选）：为空时界面回退到主机名。
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Optional — defaults to the host name"))
                .default_value(
                    config
                        .active_profile()
                        .map(|profile| profile.name.clone())
                        .unwrap_or_default(),
                )
        });
        let playback_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(0.1)
                .default_value(0.0)
        });
        let volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(config.volume * 100.0)
        });
        let initial_volume = config.volume;
        let eq_preamp = config.equalizer.preamp_db;
        let eq_preamp_slider = cx.new(move |_| {
            SliderState::new()
                .min(EqSettings::MIN_GAIN_DB)
                .max(EqSettings::MAX_GAIN_DB)
                .step(1.0)
                .default_value(eq_preamp)
        });
        let eq_sliders: Vec<Entity<SliderState>> = (0..EqSettings::BANDS.len())
            .map(|band| {
                let gain = config.equalizer.gains_db[band];
                cx.new(move |_| {
                    SliderState::new()
                        .min(EqSettings::MIN_GAIN_DB)
                        .max(EqSettings::MAX_GAIN_DB)
                        .step(1.0)
                        .default_value(gain)
                })
            })
            .collect();
        let eq_preset_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("Preset name")));

        let mut app = Self {
            logs: crate::app::settings::logs::LogViewer::new(window, cx),
            lyrics_cache: Default::default(),
            lyrics_gate: Default::default(),
            lyrics_client: None,
            lyrics_ui: crate::app::player::lyrics_search::LyricsUi::new(window, cx),
            bilibili_accounts: HashMap::new(),
            proxy_url_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("http://127.0.0.1:7890")),
            proxy_name_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("Proxy name (optional)"))),
            local_folder_name_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("Local music"))),
            local_folder_path_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("/path/to/music"))),
            webdav_name_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("Home NAS / Alist"))),
            webdav_url_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("https://nas.local:5244/dav")),
            webdav_root_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("/Music")),
            webdav_username_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("Username"))),
            webdav_password_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("Password")).masked(true)),
            runtime,
            playback_source,
            playback_server: None,
            api,
            config,
            tx,
            rx,
            audio,
            preload: PreloadState::default(),
            start_resolution: Default::default(),
            preload_resolution: Default::default(),
            media_session,
            global_shortcuts,
            default_cover,
            default_cover_thumbnails: Default::default(),
            cover_thumbnails: Default::default(),
            vinyl_cover: VinylCover::default(),
            turntable: Default::default(),
            state: AppState::default(),
            playlist_ui: PlaylistUi::default(),
            favorite_ui: Default::default(),
            favorite_writes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            import_ui: ImportUi::default(),
            search_input: search_input.clone(),
            bilibili_folder_search: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("Find a favorite folder…"))
                    .clean_on_escape()
            }),
            bilibili_search_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("Search Bilibili videos or paste a BV/AV ID or link…"))
            }),
            bilibili_parts_scroll: UniformListScrollHandle::new(),
            playlist_search_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("Search song titles, artists or albums…"))
            }),
            playlist_list_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            server_input,
            username_input,
            password_input,
            name_input,
            playback_slider: playback_slider.clone(),
            volume_slider: volume_slider.clone(),
            volume: VolumeState::new(initial_volume),
            lyrics_scroll_handle: ScrollHandle::new(),
            content_scroll_handle: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            song_list_scrolls: RefCell::new(HashMap::new()),
            pending_notifications: Vec::new(),
            cover_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            folder_cover_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            offline_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            offline_store,
            offline_outbox,
            cover_textures: RefCell::new(cover_textures),
            text_width_cache: RefCell::new(TextWidthCache::default()),
            tray_rx: mpsc::channel().1,
            track_notifier: notify::TrackNotifier::new(),
            desktop: DesktopState::default(),
            main_hwnd: None,
            quitting: Arc::new(AtomicBool::new(false)),
            shortcut_repeat: ShortcutRepeatGuard::default(),
            session: SessionState::new(std::time::Instant::now()),
            server_cache_writer: config::ServerCacheWriter::start()
                .expect("failed to start server cache writer"),
            reconnect: PlaybackReconnect::default(),
            _subscriptions: Vec::new(),
            mini_window: MiniWindowState::default(),
            fullscreen: crate::app::player::fullscreen::window::FullscreenUi::new(cx),
            eq_preamp_slider,
            eq_sliders,
            eq_preset_name_input,
            equalizer: EqualizerState::default(),
            consecutive_missing_skips: 0,
            album_page_requests: PageRequests::default(),
            library_requests: LibraryRequests::default(),
            server_removal: ServerRemovalState::default(),
        };
        if let Some(error) = cache_directory_error {
            app.notify_error(t!(
                "Unable to use cache directory %{dir}: %{error}",
                dir = requested_cache_dir.display(),
                error = error
            ));
        }
        app.state.playback_modes = app.config.playback_modes;
        app.subscribe_lyrics_inputs(cx);
        app.subscribe_log_search(cx);
        app.subscribe_playlist_search(cx);
        app.subscribe_bilibili_folder_search(cx);
        app.subscribe_bilibili_discovery(cx);
        // 封面磁盘缓存总量控制：超限时从最旧开始清理（后台线程，不阻塞启动）。
        {
            let prune_config = app.config.clone();
            app.spawn_future(async move {
                config::prune_cover_cache(&prune_config);
            });
        }
        app.state.albums_sort = app.config.albums_sort;
        app.state.artists.set_sort(app.config.artists_sort);
        app.audio.set_volume(initial_volume);
        app.audio
            .set_cache_limit_mb(app.config.audio_cache_limit_mb);
        app.audio
            .set_pause_on_output_disconnect(app.config.pause_on_output_disconnect);
        // 响度/流派/播放会话都按服务器隔离，统一在切换与启动时加载。
        app.load_active_server_cache();

        // 启动系统托盘（Windows），并拦截"关闭"改为隐藏到托盘。
        let (tray_tx, tray_rx) = mpsc::channel();
        if let Err(error) = tray::start_tray_worker(tray_tx) {
            log::warn!("failed to start tray worker: {error:#}");
        }
        app.tray_rx = tray_rx;
        app.main_hwnd = main_window_hwnd(window);

        app.subscribe_lyrics_timing_keys(cx);
        // 注册应用级快捷键监听（不依赖焦点；输入框聚焦时由输入框包装层阻止传播）。
        app._subscriptions
            .push(cx.observe_keystrokes(|this, event, window, cx| {
                this.handle_keystroke(&event.keystroke, window, cx);
            }));

        let close_quitting = app.quitting.clone();
        let close_hwnd = app.main_hwnd;
        window.on_window_should_close(cx, move |window, cx| {
            if let Some(Some(root)) = window.root::<NavidromeApp>() {
                let pending = root.read(cx).favorite_writes.load(Ordering::SeqCst) > 0;
                if pending {
                    root.update(cx, |app, _| {
                        app.notify_warning(t!(
                            "Wait for the favorite operation to finish before closing."
                        ))
                    });
                    return false;
                }
            }
            if close_quitting.load(Ordering::Relaxed) {
                // 真正退出前保存最后状态。
                if let Some(Some(root)) = window.root::<NavidromeApp>() {
                    root.update(cx, |app, _cx| {
                        app.save_playback_before_exit();
                    });
                }
                true
            } else if let Some(hwnd) = close_hwnd {
                hide_main_window(hwnd);
                false
            } else {
                // 平台层关闭（Alt+F4 等）：按设置决定退出还是最小化。
                let close_minimizes = match window.root::<NavidromeApp>() {
                    Some(Some(root)) => root.update(cx, |app, _| app.config.close_minimizes),
                    _ => false,
                };
                if close_minimizes {
                    window.minimize_window();
                    false
                } else {
                    true
                }
            }
        });
        app._subscriptions.push(
            cx.subscribe(&search_input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_search(cx);
                    cx.notify();
                }
            }),
        );
        app._subscriptions.push(cx.subscribe(
            &playback_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.cancel_next_preload();
                let duration = this.playback_for_display().duration.unwrap_or_default();
                if !duration.is_zero() {
                    this.seek_playback(duration.mul_f32((*value / 100.0).clamp(0.0, 1.0)));
                }
                cx.notify();
            },
        ));
        app._subscriptions.push(cx.subscribe(
            &volume_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.set_volume(*value / 100.0, cx);
            },
        ));
        let eq_preamp_slider = app.eq_preamp_slider.clone();
        app._subscriptions.push(cx.subscribe(
            &eq_preamp_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.set_eq_preamp(*value, cx);
            },
        ));
        let eq_sliders = app.eq_sliders.clone();
        for (band, slider) in eq_sliders.iter().enumerate() {
            app._subscriptions.push(cx.subscribe(
                slider,
                move |this, _, event: &SliderEvent, cx| {
                    let SliderEvent::Change(SliderValue::Single(value)) = event else {
                        return;
                    };
                    this.set_eq_band(band, *value, cx);
                },
            ));
        }
        app._subscriptions
            .push(cx.observe_window_appearance(window, |this, window, cx| {
                if this.config.theme == ThemePreference::System {
                    Theme::sync_system_appearance(Some(window), cx);
                    cx.notify();
                }
            }));

        if app.api.is_some() {
            app.refresh_library();
        } else if app.browsing_bilibili() {
            app.state.view = View::Home;
            app.state.status = t!("Verifying Bilibili account…").to_string();
        } else {
            app.state.status = t!("Open Settings to configure the server").to_string();
        }
        if app.config.username.trim().is_empty() && !app.browsing_bilibili() {
            app.state.status = t!("Configure your Navidrome server").to_string();
            app.state.settings.open = true;
        }

        Self::start_polling(window, cx);
        app.restore_bilibili_accounts();

        app
    }
}
