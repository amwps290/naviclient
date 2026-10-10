use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::Condvar;

/// Shared by HTTP, the decoder and its consumer. A prepared decoder sleeps
/// after its startup buffer; promotion wakes it without reopening the stream.
pub(super) struct StreamControl {
    cancelled: AtomicBool,
    state: Mutex<DecodeState>,
    wake: Condvar,
    pub(super) fully_decoded: AtomicBool,
    pub(super) speculative: bool,
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
        })
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

    fn promote(&self) {
        self.state.lock().unwrap().active = true;
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
mod tests;
