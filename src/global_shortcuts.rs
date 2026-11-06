//! 系统全局播放快捷键。Windows 使用 `RegisterHotKey` 在线程消息队列上
//! 注册；其他平台保留相同接口并由设置页明确提示暂不支持自定义全局键。

#[cfg(target_os = "windows")]
use std::sync::mpsc::Sender;
use std::sync::mpsc::{self, Receiver};

use crate::models::{GlobalShortcutSettings, ShortcutAction};

mod binding;
#[cfg(any(target_os = "windows", test))]
mod registrations;
#[cfg(target_os = "windows")]
mod windows;

pub use binding::{validate, ValidationError};
#[cfg(target_os = "windows")]
use registrations::snapshot;

#[derive(Debug)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum GlobalShortcutEvent {
    Triggered(ShortcutAction),
    RegistrationReport { failures: Vec<ShortcutAction> },
}

#[cfg(target_os = "windows")]
enum WorkerCommand {
    Update(Vec<(ShortcutAction, String)>),
    Suspend,
    Shutdown,
}

pub struct GlobalShortcutManager {
    events: Receiver<GlobalShortcutEvent>,
    #[cfg(target_os = "windows")]
    commands: Sender<WorkerCommand>,
    #[cfg(target_os = "windows")]
    worker: Option<std::thread::JoinHandle<()>>,
}

impl GlobalShortcutManager {
    pub fn new(settings: &GlobalShortcutSettings) -> Self {
        let (event_tx, events) = mpsc::channel();

        #[cfg(target_os = "windows")]
        {
            let (commands, command_rx) = mpsc::channel();
            let initial = snapshot(settings);
            let worker = std::thread::Builder::new()
                .name("global-shortcuts".to_string())
                .spawn(move || windows::run(command_rx, event_tx, initial))
                .map_err(|error| {
                    log::warn!("failed to start global shortcut worker: {error}");
                    error
                })
                .ok();
            Self {
                events,
                commands,
                worker,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            drop(event_tx);
            let _ = settings;
            Self { events }
        }
    }

    pub fn update(&self, settings: &GlobalShortcutSettings) {
        #[cfg(target_os = "windows")]
        {
            let _ = self
                .commands
                .send(WorkerCommand::Update(snapshot(settings)));
        }
        #[cfg(not(target_os = "windows"))]
        let _ = settings;
    }

    /// 录制组合键时解除旧注册，保证按键能送到当前窗口。
    pub fn suspend(&self) {
        #[cfg(target_os = "windows")]
        {
            let _ = self.commands.send(WorkerCommand::Suspend);
        }
    }

    pub fn try_recv(&self) -> Option<GlobalShortcutEvent> {
        self.events.try_recv().ok()
    }
}

#[cfg(target_os = "windows")]
impl Drop for GlobalShortcutManager {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
