use std::sync::mpsc::Sender;
#[cfg(target_os = "windows")]
use std::time::Duration;

use anyhow::Result;
#[cfg(target_os = "windows")]
use anyhow::{anyhow, Context};

/// 系统托盘菜单触发的播放器命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum TrayCommand {
    TogglePlayback,
    Previous,
    Next,
    ShowWindow,
    Quit,
}

/// 启动托盘服务。Windows：tray-icon + 独立消息泵线程；
/// Linux：ksni（纯 Rust StatusNotifierItem over D-Bus，无需 GTK）。
/// 其他平台 no-op。事件统一走 TrayCommand 通道，由应用 50ms 轮询消费。
pub fn start_tray_worker(tx: Sender<TrayCommand>) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::thread;
        thread::Builder::new()
            .name("tray-worker".to_string())
            .spawn(move || {
                if let Err(error) = run_tray_loop(&tx) {
                    log::warn!("tray worker failed: {error:#}");
                }
            })
            .map(|_| ())?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        linux::start(tx)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = tx;
        log::warn!("system tray is not implemented on this platform");
        Ok(())
    }
}

/// RGBA（R,G,B,A 每像素 4 字节）转 SNI 的 ARGB32 网络字节序
/// （A,R,G,B 每像素 4 字节）。纯转换，便于测试。
/// 目前只有 Linux 的 ksni 托盘用到；其他平台保留实现供测试与后续接入。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn rgba_to_argb32(rgba: &[u8]) -> Vec<u8> {
    let mut argb = Vec::with_capacity(rgba.len());
    for pixel in rgba.chunks_exact(4) {
        argb.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
    }
    argb
}

/// 播放/暂停状态变化时同步托盘菜单文案；托盘未启用时为 no-op。
#[cfg(target_os = "linux")]
pub fn set_tray_paused(paused: bool) {
    if let Some(handle) = linux::tray_handle() {
        handle.update(|tray| tray.set_paused(paused));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn set_tray_paused(_paused: bool) {}

/// 界面语言切换后重建托盘菜单文案。Linux（ksni）的菜单在 handle.update
/// 时重新生成，因此下一次 `menu()` 会用新语言取词；Windows 的 tray-icon
/// 菜单标签在创建时固定，改动要等下次启动才生效。
pub fn refresh_labels() {
    #[cfg(target_os = "linux")]
    {
        if let Some(handle) = linux::tray_handle() {
            handle.update(|_tray| {});
        }
    }
}

/// Linux 托盘（ksni / StatusNotifierItem over D-Bus）。
/// ksni 自带服务线程，菜单回调只做通道转发，符合其文档建议。
#[cfg(target_os = "linux")]
mod linux {
    use super::{rgba_to_argb32, TrayCommand};
    use ksni::blocking::TrayMethods as _;
    use ksni::menu::StandardItem;
    use ksni::{Icon, MenuItem, Tray};
    use rust_i18n::t;
    use std::sync::OnceLock;

    pub struct LinuxTray {
        tx: std::sync::mpsc::Sender<TrayCommand>,
        paused: bool,
        /// 播放/暂停双态图标（assets/gen_tray_icon.py 生成的圆形暮色
        /// 徽章：播放 = 八分音符，暂停 = 双竖条），各预置多个尺寸供
        /// SNI 宿主挑选，避免宿主用低质量缩放。
        icon_playing: Vec<Icon>,
        icon_paused: Vec<Icon>,
    }

    impl LinuxTray {
        /// 应用同步播放/暂停状态（菜单文案与图标一起切换）。
        pub fn set_paused(&mut self, paused: bool) {
            self.paused = paused;
        }

        /// PNG 母版（256px）高质量缩放到常见托盘尺寸并转 ARGB32。
        fn tray_icon(png: &'static [u8]) -> Vec<Icon> {
            let image = image::load_from_memory(png)
                .expect("embedded tray icon must decode")
                .to_rgba8();
            [16, 22, 24, 32, 48]
                .into_iter()
                .map(|size| {
                    let resized = image::imageops::resize(
                        &image,
                        size,
                        size,
                        image::imageops::FilterType::Lanczos3,
                    );
                    Icon {
                        width: size as i32,
                        height: size as i32,
                        data: rgba_to_argb32(&resized),
                    }
                })
                .collect()
        }
    }

    impl Tray for LinuxTray {
        fn id(&self) -> String {
            "navidrome-client".into()
        }

        fn title(&self) -> String {
            "Navidrome Client".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            if self.paused {
                self.icon_paused.clone()
            } else {
                self.icon_playing.clone()
            }
        }

        /// 左键点击托盘图标 = 播放/暂停，与常见播放器一致。
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(TrayCommand::TogglePlayback);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let paused = self.paused;
            vec![
                StandardItem {
                    label: if paused {
                        t!("Play").into()
                    } else {
                        t!("Pause").into()
                    },
                    icon_name: if paused {
                        "media-playback-start"
                    } else {
                        "media-playback-pause"
                    }
                    .into(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        let _ = tray.tx.send(TrayCommand::TogglePlayback);
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: t!("Previous").into(),
                    icon_name: "media-skip-backward".into(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        let _ = tray.tx.send(TrayCommand::Previous);
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: t!("Next").into(),
                    icon_name: "media-skip-forward".into(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        let _ = tray.tx.send(TrayCommand::Next);
                    }),
                    ..Default::default()
                }
                .into(),
                ksni::menu::MenuItem::Separator,
                StandardItem {
                    label: t!("Show window").into(),
                    icon_name: "window-new".into(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        let _ = tray.tx.send(TrayCommand::ShowWindow);
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: t!("Exit").into(),
                    icon_name: "application-exit".into(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        let _ = tray.tx.send(TrayCommand::Quit);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    static HANDLE: OnceLock<ksni::blocking::Handle<LinuxTray>> = OnceLock::new();

    pub fn tray_handle() -> Option<&'static ksni::blocking::Handle<LinuxTray>> {
        HANDLE.get()
    }

    pub fn start(tx: std::sync::mpsc::Sender<TrayCommand>) -> anyhow::Result<()> {
        let tray = LinuxTray {
            tx,
            paused: false,
            icon_playing: LinuxTray::tray_icon(include_bytes!("../assets/tray-playing.png")),
            icon_paused: LinuxTray::tray_icon(include_bytes!("../assets/tray-paused.png")),
        };
        // 无 SNI 宿主（如未装扩展的 GNOME）时 spawn 可能失败——托盘是
        // 纯增益，失败只记日志。
        match tray.spawn() {
            Ok(handle) => {
                let _ = HANDLE.set(handle);
                log::info!("system tray icon created (ksni/StatusNotifierItem)");
                Ok(())
            }
            Err(error) => {
                log::warn!("system tray unavailable: {error}");
                Ok(())
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn run_tray_loop(tx: &Sender<TrayCommand>) -> Result<()> {
    use rust_i18n::t;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    let toggle = MenuItem::new(t!("Play / Pause"), true, None);
    let previous = MenuItem::new(t!("Previous"), true, None);
    let next = MenuItem::new(t!("Next"), true, None);
    let show = MenuItem::new(t!("Show window"), true, None);
    let quit = MenuItem::new(t!("Exit"), true, None);

    let menu = Menu::new();
    menu.append(&toggle)?;
    menu.append(&previous)?;
    menu.append(&next)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&show)?;
    menu.append(&quit)?;

    let icon = Icon::from_rgba(tray_icon_rgba()?, 32, 32)
        .map_err(|error| anyhow!("invalid tray icon: {error}"))?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("Navidrome Client")
        .build()
        .map_err(|error| anyhow!("failed to create tray icon: {error}"))?;
    log::info!("system tray icon created");

    // Windows 需要泵消息循环处理托盘交互；菜单事件从 muda 全局通道转发到应用。
    loop {
        pump_windows_messages();
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let command = if event.id == toggle.id() {
                Some(TrayCommand::TogglePlayback)
            } else if event.id == previous.id() {
                Some(TrayCommand::Previous)
            } else if event.id == next.id() {
                Some(TrayCommand::Next)
            } else if event.id == show.id() {
                Some(TrayCommand::ShowWindow)
            } else if event.id == quit.id() {
                Some(TrayCommand::Quit)
            } else {
                None
            };
            if let Some(command) = command {
                let _ = tx.send(command);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "windows")]
fn pump_windows_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    unsafe {
        let mut message = std::mem::zeroed::<MSG>();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// 生成 32x32 RGBA 托盘图标（使用默认封面图）。
#[cfg(target_os = "windows")]
fn tray_icon_rgba() -> Result<Vec<u8>> {
    let bytes = include_bytes!("../assets/default-cover.png");
    let image = image::load_from_memory(bytes)
        .context("failed to decode tray icon")?
        .resize(32, 32, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Ok(image.into_raw())
}

#[cfg(test)]
mod tests {
    use super::rgba_to_argb32;

    #[test]
    fn rgba_to_argb32_reorders_channels_per_pixel() {
        // 单像素 R=1 G=2 B=3 A=4 → A=4 R=1 G=2 B=3
        assert_eq!(rgba_to_argb32(&[1, 2, 3, 4]), vec![4, 1, 2, 3]);
        // 多像素按 4 字节分组，互不串扰
        assert_eq!(
            rgba_to_argb32(&[1, 2, 3, 4, 5, 6, 7, 8]),
            vec![4, 1, 2, 3, 8, 5, 6, 7]
        );
    }
}
