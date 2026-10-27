use std::sync::{mpsc, Arc, RwLock};
use std::time::Duration;

use gpui_kit::Image;
use windows::core::{factory, HSTRING};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::WinRT::{
    ISystemMediaTransportControlsInterop, RoInitialize, RoUninitialize, RO_INIT_SINGLETHREADED,
};

use super::{track_id, MediaCommand, MediaUpdate};

#[derive(Clone, PartialEq, Eq)]
struct TrackSnapshot {
    id: String,
    title: String,
    artist: String,
    album: String,
    duration: Duration,
    cover_id: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TimelineSnapshot {
    duration: Duration,
    position_second: u64,
    seek_serial: u64,
}

struct Inner {
    controls: SystemMediaTransportControls,
    button_token: i64,
    position_token: Option<i64>,
    current_track: Arc<RwLock<Option<String>>>,
    track: Option<TrackSnapshot>,
    status: MediaPlaybackStatus,
    timeline: Option<TimelineSnapshot>,
    can_play: bool,
    can_pause: bool,
    can_next: bool,
    can_previous: bool,
    enabled: bool,
    timeline_supported: bool,
    thumbnail_stream: Option<InMemoryRandomAccessStream>,
}

pub struct MediaSession {
    inner: Option<Inner>,
    commands: mpsc::Receiver<MediaCommand>,
    restored_seek_serial: u64,
    uninitialize_winrt: bool,
}

impl MediaSession {
    pub fn new(_: &tokio::runtime::Handle, window_handle: Option<isize>) -> Self {
        let (commands, receiver) = mpsc::sync_channel(32);
        let Some(window_handle) = window_handle else {
            log::warn!("Windows media session unavailable: main window handle is missing");
            return Self {
                inner: None,
                commands: receiver,
                restored_seek_serial: 0,
                uninitialize_winrt: false,
            };
        };

        let uninitialize_winrt = match unsafe { RoInitialize(RO_INIT_SINGLETHREADED) } {
            Ok(()) => true,
            Err(error) if error.code() == RPC_E_CHANGED_MODE => false,
            Err(error) => {
                log::warn!("Windows media session unavailable (playback is unaffected): {error}");
                return Self {
                    inner: None,
                    commands: receiver,
                    restored_seek_serial: 0,
                    uninitialize_winrt: false,
                };
            }
        };

        let inner = create(window_handle, commands).map_err(|error| {
            log::warn!("Windows media session unavailable (playback is unaffected): {error}");
        });
        let initialized = inner.is_ok();
        if !initialized && uninitialize_winrt {
            unsafe { RoUninitialize() };
        }

        Self {
            inner: inner.ok(),
            commands: receiver,
            restored_seek_serial: 0,
            uninitialize_winrt: initialized && uninitialize_winrt,
        }
    }

    pub fn next_command(&self) -> Option<MediaCommand> {
        self.commands.try_recv().ok()
    }

    pub fn restored_position_changed(&mut self) {
        self.restored_seek_serial = self.restored_seek_serial.wrapping_add(1);
    }

    pub fn update(&mut self, update: MediaUpdate<'_>) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };
        if let Err(error) = inner.update(update, self.restored_seek_serial) {
            log::warn!("Windows media session update failed: {error}");
        }
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            let _ = inner.controls.RemoveButtonPressed(inner.button_token);
            if let Some(token) = inner.position_token {
                let _ = inner.controls.RemovePlaybackPositionChangeRequested(token);
            }
            let _ = inner.controls.SetIsEnabled(false);
            drop(inner);
        }
        if self.uninitialize_winrt {
            unsafe { RoUninitialize() };
        }
    }
}

fn create(
    window_handle: isize,
    commands: mpsc::SyncSender<MediaCommand>,
) -> windows::core::Result<Inner> {
    let interop: ISystemMediaTransportControlsInterop =
        factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    let controls: SystemMediaTransportControls =
        unsafe { interop.GetForWindow(HWND(window_handle as *mut std::ffi::c_void))? };

    let button_commands = commands.clone();
    let button_token = controls.ButtonPressed(&TypedEventHandler::<
        SystemMediaTransportControls,
        SystemMediaTransportControlsButtonPressedEventArgs,
    >::new(move |_, args| {
        let command = match args.ok()?.Button()? {
            SystemMediaTransportControlsButton::Play => Some(MediaCommand::Play),
            SystemMediaTransportControlsButton::Pause => Some(MediaCommand::Pause),
            SystemMediaTransportControlsButton::Stop => Some(MediaCommand::Stop),
            SystemMediaTransportControlsButton::Next => Some(MediaCommand::Next),
            SystemMediaTransportControlsButton::Previous => Some(MediaCommand::Previous),
            _ => None,
        };
        if let Some(command) = command {
            let _ = button_commands.try_send(command);
        }
        Ok(())
    }))?;

    let current_track = Arc::new(RwLock::new(None::<String>));
    let position_track = current_track.clone();
    let position_commands = commands;
    let position_token =
        match controls.PlaybackPositionChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_, args| {
            let ticks = args.ok()?.RequestedPlaybackPosition()?.Duration;
            if ticks < 0 {
                return Ok(());
            }
            let track_id = position_track
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            if let Some(track_id) = track_id {
                let _ = position_commands.try_send(MediaCommand::SetPosition {
                    track_id,
                    position: duration_from_ticks(ticks),
                });
            }
            Ok(())
        })) {
            Ok(token) => Some(token),
            Err(error) => {
                log::warn!("Windows media timeline controls unavailable: {error}");
                None
            }
        };
    let timeline_supported = position_token.is_some();

    controls.SetIsEnabled(false)?;
    controls.SetIsPlayEnabled(false)?;
    controls.SetIsPauseEnabled(false)?;
    controls.SetIsStopEnabled(false)?;
    controls.SetIsNextEnabled(false)?;
    controls.SetIsPreviousEnabled(false)?;
    controls.SetPlaybackStatus(MediaPlaybackStatus::Stopped)?;
    log::info!("Windows system media transport controls registered");

    Ok(Inner {
        controls,
        button_token,
        position_token,
        current_track,
        track: None,
        status: MediaPlaybackStatus::Stopped,
        timeline: None,
        can_play: false,
        can_pause: false,
        can_next: false,
        can_previous: false,
        enabled: false,
        timeline_supported,
        thumbnail_stream: None,
    })
}

impl Inner {
    fn update(
        &mut self,
        update: MediaUpdate<'_>,
        restored_seek_serial: u64,
    ) -> windows::core::Result<()> {
        let track = update.song.map(|song| TrackSnapshot {
            id: track_id(update.server, &song.id),
            title: song.title.clone(),
            artist: song.artist.clone(),
            album: song.album.clone(),
            duration: update.playback.duration.unwrap_or_else(|| {
                Duration::from_secs(song.duration.unwrap_or_default().max(0) as u64)
            }),
            cover_id: update.cover.map(|cover| cover.id),
        });

        if self.track != track {
            self.update_metadata(track.as_ref(), update.cover)?;
            *self
                .current_track
                .write()
                .unwrap_or_else(|error| error.into_inner()) =
                track.as_ref().map(|track| track.id.clone());
            self.track = track.clone();
            self.timeline = None;
        }

        let status = if track.is_none() || update.playback.ended || update.playback.error.is_some()
        {
            MediaPlaybackStatus::Stopped
        } else if update.playback.active && !update.playback.paused {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        };
        if self.status != status {
            self.controls.SetPlaybackStatus(status)?;
            self.status = status;
        }

        let can_play = track.is_some() && update.can_play;
        let can_pause = track.is_some()
            && update.playback.active
            && !update.playback.ended
            && update.playback.error.is_none();
        if self.can_play != can_play {
            self.controls.SetIsPlayEnabled(can_play)?;
            self.can_play = can_play;
        }
        if self.can_pause != can_pause {
            self.controls.SetIsPauseEnabled(can_pause)?;
            self.controls.SetIsStopEnabled(can_pause)?;
            self.can_pause = can_pause;
        }
        if self.can_next != update.can_next {
            self.controls.SetIsNextEnabled(update.can_next)?;
            self.can_next = update.can_next;
        }
        if self.can_previous != update.can_previous {
            self.controls.SetIsPreviousEnabled(update.can_previous)?;
            self.can_previous = update.can_previous;
        }

        let enabled = track.is_some();
        if self.enabled != enabled {
            self.controls.SetIsEnabled(enabled)?;
            self.enabled = enabled;
        }

        if let Some(track) = track {
            let timeline = TimelineSnapshot {
                duration: track.duration,
                position_second: update.position.as_secs(),
                seek_serial: update
                    .playback
                    .seek_serial
                    .wrapping_add(restored_seek_serial),
            };
            if self.timeline_supported && self.timeline != Some(timeline) {
                match self.update_timeline(track.duration, update.position) {
                    Ok(()) => self.timeline = Some(timeline),
                    Err(error) => {
                        self.timeline_supported = false;
                        log::warn!("Windows media timeline updates unavailable: {error}");
                    }
                }
            }
        }
        Ok(())
    }

    fn update_metadata(
        &mut self,
        track: Option<&TrackSnapshot>,
        cover: Option<&Arc<Image>>,
    ) -> windows::core::Result<()> {
        let updater = self.controls.DisplayUpdater()?;
        updater.ClearAll()?;
        self.thumbnail_stream = None;
        let Some(track) = track else {
            updater.Update()?;
            return Ok(());
        };

        updater.SetType(MediaPlaybackType::Music)?;
        let properties = updater.MusicProperties()?;
        properties.SetTitle(&HSTRING::from(track.title.as_str()))?;
        properties.SetArtist(&HSTRING::from(track.artist.as_str()))?;
        properties.SetAlbumTitle(&HSTRING::from(track.album.as_str()))?;

        if let Some(cover) = cover {
            match thumbnail_from_bytes(&cover.bytes) {
                Ok((reference, stream)) => {
                    updater.SetThumbnail(&reference)?;
                    self.thumbnail_stream = Some(stream);
                }
                Err(error) => log::warn!("Windows media artwork unavailable: {error}"),
            }
        }
        updater.Update()?;
        Ok(())
    }

    fn update_timeline(&self, duration: Duration, position: Duration) -> windows::core::Result<()> {
        let duration = duration.max(position);
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(TimeSpan { Duration: 0 })?;
        timeline.SetMinSeekTime(TimeSpan { Duration: 0 })?;
        timeline.SetEndTime(timespan(duration))?;
        timeline.SetMaxSeekTime(timespan(duration))?;
        timeline.SetPosition(timespan(position.min(duration)))?;
        self.controls.UpdateTimelineProperties(&timeline)
    }
}

fn thumbnail_from_bytes(
    bytes: &[u8],
) -> windows::core::Result<(RandomAccessStreamReference, InMemoryRandomAccessStream)> {
    let stream = InMemoryRandomAccessStream::new()?;
    let output = stream.GetOutputStreamAt(0)?;
    let writer = DataWriter::CreateDataWriter(&output)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.get()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    let reference = RandomAccessStreamReference::CreateFromStream(&stream)?;
    Ok((reference, stream))
}

fn timespan(duration: Duration) -> TimeSpan {
    TimeSpan {
        Duration: duration
            .as_nanos()
            .saturating_div(100)
            .min(i64::MAX as u128) as i64,
    }
}

fn duration_from_ticks(ticks: i64) -> Duration {
    Duration::from_nanos((ticks.max(0) as u64).saturating_mul(100))
}
