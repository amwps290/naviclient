//! Cross-platform process memory sampling and structured app-cache breakdown.

/// OS-reported memory counters for the current process (in bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProcessMemory {
    /// Resident Set Size (Linux `VmRSS`) / Working Set (Windows `WorkingSetSize`).
    pub rss_bytes: u64,
    /// Peak physical memory (Linux `VmHWM` / Windows `PeakWorkingSetSize`).
    pub peak_rss_bytes: Option<u64>,
    /// Private memory (Linux `RssAnon` / Windows `PrivateUsage`).
    pub private_bytes: Option<u64>,
}

/// Breakdown of tracked in-app caches and runtime structures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AppMemoryBreakdown {
    pub cover_source_count: usize,
    pub cover_source_bytes: u64,
    pub cover_texture_count: usize,
    pub cover_texture_capacity: usize,
    pub cover_thumbnail_count: usize,
    pub cover_thumbnail_bytes: u64,
    pub default_cover_bytes: u64,
    pub vinyl_bytes: u64,
    pub lyrics_bodies: usize,
    pub lyrics_bytes: u64,
    pub playlist_songs: usize,
    pub playlist_search_bytes: u64,
    pub queue_entries: usize,
    pub library_items: usize,
    pub log_buffer_bytes: u64,
}

impl AppMemoryBreakdown {
    /// Sum of all directly measured byte caches (excluding OS/GPU/allocator overhead).
    pub(crate) fn tracked_bytes(&self) -> u64 {
        self.cover_source_bytes
            .saturating_add(self.cover_thumbnail_bytes)
            .saturating_add(self.default_cover_bytes)
            .saturating_add(self.vinyl_bytes)
            .saturating_add(self.lyrics_bytes)
            .saturating_add(self.playlist_search_bytes)
            .saturating_add(self.log_buffer_bytes)
    }
}

/// Complete diagnostic memory snapshot sampled on demand or on log-panel poll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemorySnapshot {
    pub process: Option<ProcessMemory>,
    pub app: AppMemoryBreakdown,
}

impl MemorySnapshot {
    /// Remaining resident memory outside the explicitly tracked byte caches
    /// (GPUI atlas & decoded textures, graphics driver, tokio/audio threads, heap).
    pub(crate) fn untracked_rss_bytes(&self) -> Option<u64> {
        Some(
            self.process?
                .rss_bytes
                .saturating_sub(self.app.tracked_bytes()),
        )
    }
}

/// Sample OS process memory counters for the current process.
pub(crate) fn sample_process_memory() -> Option<ProcessMemory> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        parse_proc_status(&status)
    }
    #[cfg(target_os = "windows")]
    {
        sample_windows_memory()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_proc_status(status: &str) -> Option<ProcessMemory> {
    let mut rss_kb = None;
    let mut hwm_kb = None;
    let mut anon_kb = None;
    for line in status.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let value = rest
            .split_ascii_whitespace()
            .next()
            .and_then(|token| token.parse::<u64>().ok());
        match key {
            "VmRSS" => rss_kb = value,
            "VmHWM" => hwm_kb = value,
            "RssAnon" => anon_kb = value,
            _ => {}
        }
    }
    let rss_bytes = rss_kb?.saturating_mul(1024);
    Some(ProcessMemory {
        rss_bytes,
        peak_rss_bytes: hwm_kb.map(|kb| kb.saturating_mul(1024)),
        private_bytes: anon_kb.map(|kb| kb.saturating_mul(1024)),
    })
}

#[cfg(target_os = "windows")]
fn sample_windows_memory() -> Option<ProcessMemory> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = zeroed();
        counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        );
        if ok == 0 {
            return None;
        }
        Some(ProcessMemory {
            rss_bytes: counters.WorkingSetSize as u64,
            peak_rss_bytes: Some(counters.PeakWorkingSetSize as u64),
            private_bytes: Some(counters.PrivateUsage as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_proc_status_memory_fields() {
        let sample = "\
Name:\tnavidrome-client
VmPeak:\t  987654 kB
VmSize:\t  654321 kB
VmHWM:\t  210000 kB
VmRSS:\t  145600 kB
RssAnon:\t   98400 kB
RssFile:\t   47200 kB
";
        let parsed = parse_proc_status(sample).expect("parsed proc status");
        assert_eq!(parsed.rss_bytes, 145_600 * 1024);
        assert_eq!(parsed.peak_rss_bytes, Some(210_000 * 1024));
        assert_eq!(parsed.private_bytes, Some(98_400 * 1024));
    }

    #[test]
    fn missing_vm_rss_returns_none() {
        assert!(parse_proc_status("Name:\tfoo\nVmSize:\t100 kB\n").is_none());
    }

    #[test]
    fn untracked_rss_saturates_when_tracked_exceeds_rss() {
        let snap = MemorySnapshot {
            process: Some(ProcessMemory {
                rss_bytes: 100,
                peak_rss_bytes: Some(200),
                private_bytes: Some(80),
            }),
            app: AppMemoryBreakdown {
                cover_source_bytes: 70,
                lyrics_bytes: 60,
                ..Default::default()
            },
        };
        assert_eq!(snap.app.tracked_bytes(), 130);
        assert_eq!(snap.untracked_rss_bytes(), Some(0));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_live_process_memory_reads_nonzero_rss() {
        let mem = sample_process_memory().expect("linux /proc/self/status must be readable");
        assert!(mem.rss_bytes > 0);
    }
}
