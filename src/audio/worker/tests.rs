use super::run_playback;
use crate::audio::device_output::{AudioOutput, DevicePlayback};
use crate::audio::preload::tests::{wait_for, wav, TestCache};
use crate::audio::stream_cache::cache_paths;
use crate::audio::transport::Transport;
use crate::audio::AudioHandle;
use crate::audio::PlaybackState;
use reqwest::blocking::Client;
use rodio::{ChannelCount, Player, SampleRate};
use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn stop_cancels_a_preparing_stream_before_late_http_bytes_reach_cache() {
    assert_pending_http_cancelled(false);
}

#[test]
fn worker_shutdown_cancels_preparation_without_waiting_for_blocked_http() {
    assert_pending_http_cancelled(true);
}

fn assert_pending_http_cancelled(shutdown: bool) {
    let cache = TestCache::new();
    let (complete, partial) = cache_paths(&cache.0, "late-stream");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/audio", listener.local_addr().unwrap());
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        let data = wav();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let _ = stream.write_all(&data);
        // Closing the client response proves its reader has finished, so the
        // cache assertion cannot race the first background read after release.
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                ) => {}
            other => panic!("client response was not released: {other:?}"),
        }
    });
    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(PlaybackState::default()));
    let audio = AudioHandle {
        tx,
        state: state.clone(),
        transition: Arc::new(Mutex::new(None)),
        transport: Arc::new(Mutex::new(Transport::default())),
    };
    let (player, _output) = Player::new();
    let output = AudioOutput::new(DevicePlayback::headless(
        player,
        None,
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
    ));
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let path = cache.0.clone();
    let worker = thread::spawn(move || run_playback(rx, state, path, client, output));
    audio.play(url, "late-stream".into(), None, 1.0, Duration::ZERO, None);
    wait_for(|| partial.exists());
    assert!(audio.state().active);
    assert!(!audio.progress().active);
    assert_eq!(fs::metadata(&partial).unwrap().len(), 0);
    let mut running = Some((audio, worker));
    if shutdown {
        let (audio, worker) = running.take().unwrap();
        drop(audio);
        let (exited_tx, exited_rx) = mpsc::channel();
        let joiner = thread::spawn(move || {
            worker.join().unwrap();
            exited_tx.send(()).unwrap();
        });
        // The worker must leave before the server releases its blocked body.
        exited_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        joiner.join().unwrap();
    } else {
        let (audio, _) = running.as_ref().unwrap();
        audio.stop();
        wait_for(|| !audio.state.lock().unwrap().active);
    }
    release_tx.send(()).unwrap();
    server.join().unwrap();
    if let Some((audio, worker)) = running.take() {
        drop(audio);
        worker.join().unwrap();
    }
    let cached_bytes = fs::metadata(&partial)
        .or_else(|_| fs::metadata(&complete))
        .unwrap()
        .len();
    assert_eq!(
        cached_bytes, 0,
        "cancelled preparation wrote late audio bytes"
    );
    assert!(!complete.exists());
}
