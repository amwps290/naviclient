//! 单实例支持：确保同一时间只有一个应用实例。
//! 第二个实例启动时会向已运行实例发送"激活窗口"信号后退出。
//!
//! Windows：命名互斥体检测 + 命名事件通知激活；其他平台暂为 no-op。

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject,
    };

    const MUTEX_NAME: &str = "Local\\NavidromeClient.SingleInstance";
    const ACTIVATE_EVENT_NAME: &str = "Local\\NavidromeClient.Activate";

    /// 句柄以 `isize` 存储：裸指针（`*mut c_void`）不满足 Send/Sync，无法放入
    /// `static OnceLock`；句柄本身是指针大小且只在本线程创建/关闭，转为 isize 后
    /// 天然满足 Send/Sync。使用时再转回 HANDLE。
    struct Guard {
        /// 持有互斥体句柄以保持单例锁；Drop 时释放。
        _mutex: isize,
        activate_event: isize,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self._mutex as HANDLE);
                CloseHandle(self.activate_event as HANDLE);
            }
        }
    }

    static INSTANCE: OnceLock<Guard> = OnceLock::new();

    /// 尝试成为单例；返回是否已有实例在运行。
    /// 若已有实例，向其发送"激活窗口"信号。
    pub fn acquire() -> bool {
        unsafe {
            let name = wide(MUTEX_NAME);
            let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if GetLastError() == ERROR_ALREADY_EXISTS {
                // 已有实例在运行：触发其激活事件后返回。
                if !mutex.is_null() {
                    CloseHandle(mutex);
                }
                let event_name = wide(ACTIVATE_EVENT_NAME);
                let event = CreateEventW(std::ptr::null(), 0, 0, event_name.as_ptr());
                if !event.is_null() {
                    SetEvent(event);
                    CloseHandle(event);
                }
                true
            } else {
                let event_name = wide(ACTIVATE_EVENT_NAME);
                let event = CreateEventW(std::ptr::null(), 0, 0, event_name.as_ptr());
                let _ = INSTANCE.set(Guard {
                    _mutex: mutex as isize,
                    activate_event: event as isize,
                });
                false
            }
        }
    }

    /// 非阻塞检查是否收到"激活窗口"信号；供主循环轮询。
    pub fn poll_activation() -> bool {
        let Some(guard) = INSTANCE.get() else {
            return false;
        };
        unsafe { WaitForSingleObject(guard.activate_event as HANDLE, 0) == WAIT_OBJECT_0 }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub fn acquire() -> bool {
        false
    }

    pub fn poll_activation() -> bool {
        false
    }
}

/// 尝试成为单例；返回是否已有实例在运行（已有实例会被通知激活窗口）。
pub fn acquire() -> bool {
    imp::acquire()
}

/// 检查是否收到"激活窗口"信号（主循环轮询）。
pub fn poll_activation() -> bool {
    imp::poll_activation()
}
