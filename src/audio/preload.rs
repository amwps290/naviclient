use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Condvar;

/// 单调递增的毫秒时钟，用于在多个线程间比较「数据上次到达」的时间。
fn now_ms() -> u64 {
    static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let origin = ORIGIN.get_or_init(Instant::now);
    u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Shared by HTTP, the decoder and its consumer. A prepared decoder sleeps
/// after its startup buffer; promotion wakes it without reopening the stream.
pub(super) struct StreamControl {
    cancelled: AtomicBool,
    state: Mutex<DecodeState>,
    wake: Condvar,
    pub(super) fully_decoded: AtomicBool,
    pub(super) speculative: bool,
    /// 最近一次向播放器交付解码数据的时刻（`now_ms`），用于检测播放停滞。
    last_data_ms: AtomicU64,
    /// 播放器为等待网络而消耗的人工静音帧。底层输出时钟会把这些帧
    /// 当成正常音频时间，状态层需要扣除它们，避免进度在缓冲时继续前进。
    synthetic_silence_frames: AtomicU64,
    synthetic_silence_rate: AtomicU64,
    /// 读取链路上最后一次失败的原因；解码器吞掉 I/O 错误时靠它还原现场。
    last_io_error: Mutex<Option<FailureError>>,
}

struct DecodeState {
    active: bool,
    running: bool,
}

pub(super) struct DecoderGuard(Arc<StreamControl>);

impl Drop for DecoderGuard {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().running = false;
        self.0.wake.notify_all();
    }
}

impl StreamControl {
    pub(super) fn new(speculative: bool) -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            state: Mutex::new(DecodeState {
                active: !speculative,
                running: false,
            }),
            wake: Condvar::new(),
            fully_decoded: AtomicBool::new(false),
            speculative,
            last_data_ms: AtomicU64::new(now_ms()),
            synthetic_silence_frames: AtomicU64::new(0),
            synthetic_silence_rate: AtomicU64::new(0),
            last_io_error: Mutex::new(None),
        })
    }

    /// 记录一次数据交付；播放器只要拿到数据，停滞计时就归零。
    pub(super) fn mark_data(&self) {
        self.last_data_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// 数据已经停止流入多久。
    pub(super) fn idle(&self) -> Duration {
        Duration::from_millis(now_ms().saturating_sub(self.last_data_ms.load(Ordering::Relaxed)))
    }

    /// 记录一次为了等待解码数据而注入的静音帧（每声道一组样本算一帧）。
    pub(super) fn mark_synthetic_silence_frame(&self, sample_rate: SampleRate) {
        self.synthetic_silence_rate
            .store(u64::from(sample_rate.get()), Ordering::Relaxed);
        self.synthetic_silence_frames
            .fetch_add(1, Ordering::Relaxed);
    }

    /// 人工静音占用的播放时钟时间；真实曲目位置应扣除这部分。
    pub(super) fn synthetic_silence_duration(&self) -> Duration {
        let frames = self.synthetic_silence_frames.load(Ordering::Relaxed);
        let rate = self.synthetic_silence_rate.load(Ordering::Relaxed);
        if frames == 0 || rate == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(frames as f64 / rate as f64)
        }
    }

    /// 供读取链路上报失败原因；保留第一次（通常最接近根因）。
    pub(super) fn record_io_error(&self, error: FailureError) {
        if let Ok(mut slot) = self.last_io_error.lock() {
            if slot.is_none() {
                *slot = Some(error);
            }
        }
    }

    pub(super) fn take_io_error(&self) -> Option<FailureError> {
        self.last_io_error
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    pub(super) fn check(&self) -> io::Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(io::Error::other("audio preparation cancelled"))
        } else {
            Ok(())
        }
    }

    pub(super) fn cancel(&self) {
        let _guard = self.state.lock().unwrap();
        self.cancelled.store(true, Ordering::Release);
        self.wake.notify_all();
    }

    pub(super) fn promote(&self) {
        self.state.lock().unwrap().active = true;
        // 预加载可能已经等了一会儿；从「开始播放」重新计时，避免刚 promot
        // 就显示成停滞。
        self.mark_data();
        self.wake.notify_all();
    }

    pub(super) fn wait_for_decode(&self, chunks: usize) -> bool {
        if chunks >= STARTUP_BUFFER_CHUNKS {
            let mut state = self.state.lock().unwrap();
            while !state.active && self.check().is_ok() {
                state = self.wake.wait(state).unwrap();
            }
        }
        self.check().is_ok()
    }

    pub(super) fn decoder_guard(self: &Arc<Self>) -> DecoderGuard {
        self.state.lock().unwrap().running = true;
        DecoderGuard(self.clone())
    }

    fn wait_until_reusable(&self) {
        let mut state = self.state.lock().unwrap();
        // A cancelled startup may leave its decoder inside a blocking read.
        // Wait here before starting another job, so rapid edits cannot accumulate
        // detached HTTP/decoder threads. A promoted decoder belongs to playback.
        while state.running && !state.active {
            state = self.wake.wait(state).unwrap();
        }
    }
}

struct Job {
    id: u64,
    url: String,
    cache_key: String,
    cache_dir: PathBuf,
    duration: Option<Duration>,
    control: Arc<StreamControl>,
}

#[derive(Default)]
struct Mailbox {
    job: Option<Job>,
    closed: bool,
}

struct Entry {
    id: u64,
    cache_key: String,
    control: Arc<StreamControl>,
    ready: Option<PreparedStream>,
}

/// One preparation thread and one replaceable pending job, even when the user
/// edits the queue rapidly. A cancelled blocking HTTP call is bounded by the
/// client's I/O timeout and cannot hold up foreground playback.
pub(super) struct Preloader {
    mailbox: Arc<(Mutex<Mailbox>, Condvar)>,
    results: Receiver<(u64, Result<PreparedStream>)>,
    entry: Option<Entry>,
}

impl Preloader {
    pub(super) fn new(client: Client) -> Result<Self> {
        let mailbox = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let worker_mailbox = mailbox.clone();
        let (tx, results) = mpsc::channel();
        thread::Builder::new()
            .name("audio-preload".into())
            .spawn(move || loop {
                let job = {
                    let (lock, wake) = &*worker_mailbox;
                    let mut pending = lock.lock().unwrap();
                    while pending.job.is_none() && !pending.closed {
                        pending = wake.wait(pending).unwrap();
                    }
                    if pending.closed {
                        break;
                    }
                    pending.job.take().unwrap()
                };
                let result = play_url(
                    client.clone(),
                    &job.url,
                    &job.cache_dir,
                    &job.cache_key,
                    job.duration,
                    Duration::ZERO,
                    job.control.clone(),
                );
                if job.control.check().is_ok() && tx.send((job.id, result)).is_err() {
                    break;
                }
                job.control.wait_until_reusable();
            })
            .context("failed to start audio preload thread")?;
        Ok(Self {
            mailbox,
            results,
            entry: None,
        })
    }

    pub(super) fn request(
        &mut self,
        id: u64,
        url: String,
        cache_key: String,
        duration: Option<Duration>,
        cache_dir: PathBuf,
    ) {
        self.cancel();
        let control = StreamControl::new(true);
        self.entry = Some(Entry {
            id,
            cache_key: cache_key.clone(),
            control: control.clone(),
            ready: None,
        });
        let (lock, wake) = &*self.mailbox;
        lock.lock().unwrap().job = Some(Job {
            id,
            url,
            cache_key,
            cache_dir,
            duration,
            control,
        });
        wake.notify_one();
    }

    pub(super) fn poll(&mut self) {
        while let Ok((id, result)) = self.results.try_recv() {
            if let Some(entry) = self.entry.as_mut().filter(|entry| entry.id == id) {
                match result {
                    Ok(stream) => {
                        if stream.source.terminal_error.is_some() {
                            log::debug!("next-track preload ended prematurely; ordinary playback will retry");
                            entry.control.cancel();
                        } else {
                            log::debug!("next-track startup buffer ready; ticket={id}");
                            entry.ready = Some(stream);
                        }
                    }
                    Err(error) => {
                        log::debug!(
                            "next-track preload failed; ordinary playback will retry: {error:#}"
                        );
                        entry.control.cancel();
                    }
                }
            }
        }
    }

    pub(super) fn take(
        &mut self,
        id: Option<u64>,
        cache_key: &str,
        position: Duration,
    ) -> Option<PreparedStream> {
        self.poll();
        let mut entry = self.entry.take()?;
        if id == Some(entry.id) && entry.cache_key == cache_key && position.is_zero() {
            if let Some(mut stream) = entry.ready.take() {
                stream.source.last_data = Instant::now();
                entry.control.promote();
                log::info!("playing preloaded audio; ticket={}", entry.id);
                return Some(stream);
            }
        }
        entry.control.cancel();
        None
    }

    pub(super) fn take_ready(&mut self) -> Option<PreparedStream> {
        self.poll();
        self.entry.as_ref()?.ready.as_ref()?;
        self.entry.take()?.ready
    }

    pub(super) fn cancel(&mut self) {
        if let Some(entry) = self.entry.take() {
            entry.control.cancel();
        }
        self.mailbox.0.lock().unwrap().job = None;
        self.poll();
    }
}

impl Drop for Preloader {
    fn drop(&mut self) {
        self.cancel();
        let (lock, wake) = &*self.mailbox;
        lock.lock().unwrap().closed = true;
        wake.notify_one();
    }
}

#[cfg(test)]
pub(super) mod tests;
