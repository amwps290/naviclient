//! Linux SNI lifetime, playback state, menu callbacks and icon publication.

use super::pixels::rgba_to_argb32;
use super::TrayCommand;
use ksni::blocking::TrayMethods as _;
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Tray};
use rust_i18n::t;
use std::sync::OnceLock;

struct LinuxTray {
    tx: std::sync::mpsc::Sender<TrayCommand>,
    paused: bool,
    /// 播放/暂停双态图标（与应用主图标共用黑胶 + 唱针标记，唱片
    /// 中心以播放/暂停符号区分状态），各预置多个尺寸供 SNI 宿主
    /// 挑选，避免宿主用低质量缩放。
    icon_playing: Vec<Icon>,
    icon_paused: Vec<Icon>,
}

impl LinuxTray {
    fn new(tx: std::sync::mpsc::Sender<TrayCommand>) -> Self {
        Self {
            tx,
            paused: false,
            icon_playing: Self::tray_icon(include_bytes!("../../assets/tray-playing.png")),
            icon_paused: Self::tray_icon(include_bytes!("../../assets/tray-paused.png")),
        }
    }

    /// 应用同步播放/暂停状态（菜单文案与图标一起切换）。
    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    fn menu_item(label: String, icon_name: &str, command: TrayCommand) -> MenuItem<Self> {
        StandardItem {
            label,
            icon_name: icon_name.into(),
            activate: Box::new(move |tray: &mut LinuxTray| {
                let _ = tray.tx.send(command);
            }),
            ..Default::default()
        }
        .into()
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
            Self::menu_item(
                if paused {
                    t!("Play").into()
                } else {
                    t!("Pause").into()
                },
                if paused {
                    "media-playback-start"
                } else {
                    "media-playback-pause"
                },
                TrayCommand::TogglePlayback,
            ),
            Self::menu_item(
                t!("Previous").into(),
                "media-skip-backward",
                TrayCommand::Previous,
            ),
            Self::menu_item(t!("Next").into(), "media-skip-forward", TrayCommand::Next),
            MenuItem::Separator,
            Self::menu_item(
                t!("Show window").into(),
                "window-new",
                TrayCommand::ShowWindow,
            ),
            Self::menu_item(t!("Exit").into(), "application-exit", TrayCommand::Quit),
        ]
    }
}

static HANDLE: OnceLock<ksni::blocking::Handle<LinuxTray>> = OnceLock::new();

pub(super) fn set_paused(paused: bool) {
    if let Some(handle) = HANDLE.get() {
        handle.update(|tray| tray.set_paused(paused));
    }
}

pub(super) fn refresh_labels() {
    if let Some(handle) = HANDLE.get() {
        handle.update(|_tray| {});
    }
}

pub(super) fn start(tx: std::sync::mpsc::Sender<TrayCommand>) -> anyhow::Result<()> {
    let tray = LinuxTray::new(tx);
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

#[cfg(test)]
mod tests;
