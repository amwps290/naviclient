//! Worker command dispatch, transport acknowledgement, and seek coalescing.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use rust_i18n::t;

use super::playback::publish_track;
use super::{Command, PlaybackWorker, ReadyOutput};
use crate::audio::error::{FailureError, StreamFailure};
use crate::audio::next_track::PreloadRequest;
use crate::audio::preparation::StreamRequest;
use crate::audio::preparation_state::PreparationKind;
use crate::audio::set_state;
use crate::audio::stream_cache::prepare_cache_dir;
use crate::audio::stream_response::stream_endpoint;
use crate::audio::transport::WorkerCommand;

const SEEK_DEBOUNCE: Duration = Duration::from_millis(180);

pub(in crate::audio) fn coalesce_seek_commands(
    initial: Duration,
    serial: Option<u64>,
    rx: &Receiver<WorkerCommand>,
    pending_commands: &mut VecDeque<WorkerCommand>,
) -> (Duration, Option<u64>, usize) {
    let mut latest = initial;
    let mut latest_serial = serial;
    let mut coalesced = 1;
    let mut deadline = Instant::now() + SEEK_DEBOUNCE;

    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok(WorkerCommand {
                command: Command::Seek(position),
                serial,
            }) => {
                latest = position;
                latest_serial = serial;
                coalesced += 1;
                deadline = Instant::now() + SEEK_DEBOUNCE;
            }
            Ok(command) => {
                pending_commands.push_back(command);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    (latest, latest_serial, coalesced)
}

impl PlaybackWorker {
    pub(super) fn handle_command(&mut self, command: WorkerCommand) -> Option<ReadyOutput> {
        let WorkerCommand {
            command,
            mut serial,
        } = command;
        let mut ready_output = None;
        match command {
            Command::Play {
                url,
                cache_key,
                duration_hint,
                start,
                gain,
            } => {
                let queued =
                    self.next_track
                        .begin_play(start.preload_id, &cache_key, start.position);
                log::info!(
                    "play requested; endpoint={} duration_hint_ms={:?} gain={gain}",
                    stream_endpoint(&url),
                    duration_hint.map(|duration| duration.as_millis())
                );
                let start_position = self.current.begin_play(
                    url.clone(),
                    cache_key.clone(),
                    duration_hint,
                    gain,
                    start.position,
                );
                let token =
                    self.preparation
                        .begin(PreparationKind::Play, start_position, Instant::now());
                self.preparation.set_paused(start.paused);
                self.output.device.player.stop();
                self.output.device.player.clear();
                publish_track(
                    &self.state,
                    &self.current,
                    self.preparation.state(),
                    &self.output.device.player,
                    false,
                );
                let preloaded =
                    self.next_track
                        .take_prepared(start.preload_id, &cache_key, start_position);
                if let Some(queued) = queued {
                    queued.info.control.promote();
                    self.preparation.complete(token);
                    ready_output = Some((token, queued));
                } else if let Some(stream) = preloaded {
                    self.preparation.deliver(token, Ok(stream));
                } else {
                    self.preparation.spawn(
                        token,
                        self.client.clone(),
                        StreamRequest {
                            url,
                            cache_key,
                            cache_dir: self.cache_dir.clone(),
                            duration: duration_hint,
                        },
                    );
                }
            }
            Command::Preload {
                id,
                url,
                cache_key,
                duration_hint,
                gain,
                transition,
            } => {
                self.next_track.reserve(PreloadRequest {
                    id,
                    url,
                    cache_key,
                    duration: duration_hint,
                    gain,
                    transition,
                });
            }
            Command::CancelPreload => {
                self.next_track.cancel();
            }
            Command::Pause => {
                log::debug!("playback pause requested");
                self.preparation.set_paused(true);
                self.output.device.player.pause();
                set_state(&self.state, |s| s.paused = true);
            }
            Command::Resume => {
                log::debug!("playback resume requested");
                self.preparation.set_paused(false);
                self.output.device.player.play();
                set_state(&self.state, |s| s.paused = false);
            }
            Command::Stop => {
                self.next_track.cancel();
                log::debug!("playback stop requested");
                self.preparation.stop();
                self.output.device.player.stop();
                self.output.device.player.clear();
                self.current.stop();
                publish_track(
                    &self.state,
                    &self.current,
                    self.preparation.state(),
                    &self.output.device.player,
                    false,
                );
            }
            Command::SetVolume(new_volume) => {
                self.volume = new_volume.clamp(0.0, 1.0);
                self.output.device.player.set_volume(self.volume);
            }
            Command::SetCacheDirectory(new_cache_dir) => {
                self.next_track.cancel();
                match prepare_cache_dir(&new_cache_dir) {
                    Ok(()) => {
                        log::info!(
                            "audio cache directory changed; directory={}",
                            new_cache_dir.display()
                        );
                        self.cache_dir = new_cache_dir;
                    }
                    Err(error) => log::error!(
                        "failed to change audio cache directory; directory={}: {error:#}",
                        new_cache_dir.display()
                    ),
                }
            }
            Command::SetOutputDevice(target) => {
                // 先记录当前输出进度与出声状态（旧 Player 释放前）。
                let had_output = !self.output.device.player.empty();
                let position = self.current.position(self.output.device.player.get_pos());
                match self.output.switch_device(target.as_deref(), &self.state) {
                    Ok(()) => {
                        // 正在出声的曲目：在新输出上从当前位置继续。
                        // 复用 Seek 路径重建流（保留暂停意图、作废预
                        // 加载衔接）；准备中的曲目会在新 Player 上落地。
                        if had_output {
                            self.pending_commands
                                .push_back(Command::Seek(position).into());
                        }
                    }
                    Err(error) => {
                        // 保留原输出继续播放；空闲时错误会经由启动错误显示。
                        log::warn!("failed to switch audio output to {target:?}: {error:#}");
                        self.current.record_error(FailureError::new(
                            StreamFailure::Output,
                            t!(
                                "unable to switch audio output: %{error}",
                                error = format!("{error:#}")
                            )
                            .to_string(),
                        ));
                    }
                }
            }
            Command::SetEqualizer(settings) => {
                // 共享状态 + 版本号：滤波器在下一个检查点重算系数。
                self.output.eq.set(settings);
            }
            Command::Seek(position) => {
                self.next_track.cancel();
                let (position, latest_serial, coalesced) =
                    coalesce_seek_commands(position, serial, &self.rx, &mut self.pending_commands);
                serial = latest_serial;
                if let Some((target, request)) =
                    self.current.prepare_seek(position, self.cache_dir.clone())
                {
                    log::info!(
                        "playback seek requested; target_ms={} coalesced_events={coalesced}",
                        target.as_millis()
                    );
                    let token =
                        self.preparation
                            .begin(PreparationKind::Seek, target, Instant::now());
                    // Only explicit commands determine pause intent while
                    // the player is cleared and its stream is rebuilt.
                    self.output.device.player.stop();
                    self.output.device.player.clear();
                    publish_track(
                        &self.state,
                        &self.current,
                        self.preparation.state(),
                        &self.output.device.player,
                        false,
                    );
                    self.preparation.spawn(token, self.client.clone(), request);
                }
            }
        }
        if let Some(serial) = serial {
            self.command_serial = serial;
        }

        ready_output
    }
}
