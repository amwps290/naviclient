use super::super::error::classify_anyhow;
use super::*;
use crate::audio::buffered_stream::STARTUP_BUFFER_CHUNKS;
use crate::audio::http_stream::tests::serve;
use crate::audio::preload::tests::{client, wait_for, wav, TestCache};
use crate::audio::stream_cache::cache_paths;
use std::fs;

#[test]
fn header_resource_decodes_pcm_with_a_bounded_temporary_cache() {
    use crate::audio::http_stream::resource_tests::{resource, Fixture};
    let cache = TestCache::new();
    let fixture = Fixture::audio(wav());
    let mut http = resource(fixture.url());
    http = http.with_mime_hint(Some("audio/wav".into()));
    let control = StreamControl::new(false);
    let prepared = play_resource(
        client(),
        http,
        &cache.0,
        "resource",
        None,
        Duration::ZERO,
        control.clone(),
    )
    .unwrap();
    let error = prepared.error.clone();
    wait_for(|| Arc::strong_count(&control) == 2);
    assert_eq!(prepared.source.count(), 12 * 48_000 * 2);
    assert!(control.fully_decoded());
    assert!(error.lock().unwrap().is_none());
    assert_eq!(fs::read_dir(&cache.0).unwrap().count(), 0);
    assert!(!fixture.requests().is_empty());
    for request in fixture.requests() {
        assert_eq!(request["referer"], "https://www.bilibili.com/");
    }
}

/// Run explicitly with a previously fetched independent AAC-LC sample.
/// No online/API requests or credentials are needed by this development probe.
#[test]
#[ignore = "set NAVICLIENT_AUDIO_PROBE_FILE to a local AAC-LC sample"]
fn independent_aac_lc_sample_decodes_and_seeks_via_application_http_reader() {
    use crate::audio::http_stream::resource_tests::{resource, Fixture};
    let path = std::env::var_os("NAVICLIENT_AUDIO_PROBE_FILE").expect("missing sample path");
    let bytes = fs::read(path).expect("could not read independent audio sample");
    let fixture = Fixture::audio(bytes);
    let cache = TestCache::new();
    for position in [0, 120, 15, 200] {
        let mut http = resource(fixture.url());
        http = http.with_mime_hint(Some("audio/mp4".into()));
        let control = StreamControl::new(false);
        let prepared = play_resource(
            client(),
            http,
            &cache.0,
            "aac-probe",
            Some(Duration::from_secs(213)),
            Duration::from_secs(position),
            control.clone(),
        )
        .unwrap();
        assert_eq!(prepared.source.channels().get(), 2);
        assert_eq!(prepared.source.sample_rate().get(), 48_000);
        let error = prepared.error.clone();
        let (count, nonzero, finite) =
            prepared
                .source
                .fold((0_u64, false, true), |(count, nonzero, finite), sample| {
                    (
                        count + 1,
                        nonzero || sample.abs() > 0.00001,
                        finite && sample.is_finite(),
                    )
                });
        // The nonblocking production source can inject buffering silence when
        // consumed faster than an output device. Count decoded PCM separately.
        let synthetic =
            (control.synthetic_silence_duration().as_secs_f64() * 96_000.0).round() as u64;
        let pcm = count.saturating_sub(synthetic);
        assert!(pcm > 48_000 * 2 && nonzero && finite);
        assert!(control.fully_decoded());
        assert!(error.lock().unwrap().is_none());
        println!("AAC-LC application reader: start={position}s PCM_samples={pcm} decoded=true");
    }
    assert_eq!(fs::read_dir(&cache.0).unwrap().count(), 0);
    let requests = fixture.requests();
    assert!(requests
        .iter()
        .any(|request| request["range"] != "bytes=0-"));
    for request in requests {
        assert_eq!(request["referer"], "https://www.bilibili.com/");
        assert!(!request.contains_key("cookie"));
    }
}

#[test]
fn reported_io_failure_does_not_verify_a_decoded_stream() {
    let cache = TestCache::new();
    fs::write(cache_paths(&cache.0, "reported-failure").0, wav()).unwrap();
    let control = StreamControl::new(false);
    control.record_io_error(FailureError::new(
        StreamFailure::Truncated,
        "reported range failure",
    ));
    let prepared = play_url(
        client(),
        "http://127.0.0.1:1/unused",
        &cache.0,
        "reported-failure",
        None,
        Duration::ZERO,
        control.clone(),
    )
    .unwrap();
    let error = prepared.error.clone();
    // Wait for the decoder to exit before pulling faster than a real device,
    // so the assertion counts PCM rather than temporary buffering silence.
    wait_for(|| Arc::strong_count(&control) == 2);
    assert_eq!(prepared.source.count(), 12 * 48_000 * 2);
    assert_eq!(
        error.lock().unwrap().as_ref().unwrap().kind,
        StreamFailure::Truncated
    );
    assert!(!control.fully_decoded());
}

#[test]
fn empty_decoded_stream_is_a_decode_failure_instead_of_a_network_timeout() {
    let cache = TestCache::new();
    let mut bytes = wav();
    bytes.truncate(44);
    bytes[4..8].copy_from_slice(&36_u32.to_le_bytes());
    bytes[40..44].copy_from_slice(&0_u32.to_le_bytes());
    fs::write(cache_paths(&cache.0, "empty").0, bytes).unwrap();
    let result = play_url(
        client(),
        "http://127.0.0.1:1/unused",
        &cache.0,
        "empty",
        None,
        Duration::ZERO,
        StreamControl::new(false),
    );
    let error = match result {
        Ok(_) => panic!("prepared audio with no samples"),
        Err(error) => error,
    };
    assert_eq!(classify_anyhow(&error), StreamFailure::Decode);
}

#[test]
fn a_truncated_decoder_probe_preserves_the_network_failure() {
    let cache = TestCache::new();
    let bytes = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nContent-Type: audio/wav\r\nConnection: close\r\n\r\nRIFF";
    let (url, server) = serve(vec![(0, bytes)]);
    let result = play_url(
        client(),
        &url,
        &cache.0,
        "truncated-probe",
        None,
        Duration::ZERO,
        StreamControl::new(false),
    );
    server.join().unwrap();
    let error = match result {
        Ok(_) => panic!("decoded an incomplete header"),
        Err(error) => error,
    };
    assert_eq!(classify_anyhow(&error), StreamFailure::Truncated);
}

#[test]
fn invalid_binary_audio_keeps_its_nonretryable_decode_classification() {
    let cache = TestCache::new();
    fs::write(
        cache_paths(&cache.0, "invalid-binary").0,
        b"unsupported binary audio",
    )
    .unwrap();
    let result = play_url(
        client(),
        "http://127.0.0.1:1/unused",
        &cache.0,
        "invalid-binary",
        None,
        Duration::ZERO,
        StreamControl::new(false),
    );
    let error = match result {
        Ok(_) => panic!("decoded invalid audio"),
        Err(error) => error,
    };
    let kind = classify_anyhow(&error);
    assert_eq!(kind, StreamFailure::Decode);
    assert!(!kind.is_retryable());
}

fn samples(count: usize) -> rodio::buffer::SamplesBuffer {
    rodio::buffer::SamplesBuffer::new(
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
        vec![0.25; count],
    )
}

#[test]
fn decoder_spawn_failure_is_local_nonretryable_and_releases_its_task() {
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::Other] {
        let control = StreamControl::new(false);
        let result = spawn_with(samples(2), None, control.clone(), |_task| {
            Err(io::Error::from(kind))
        });
        let error = match result {
            Ok(_) => panic!("decoder unexpectedly spawned"),
            Err(error) => error,
        };
        let failure = classify_anyhow(&error);
        assert_eq!(failure, StreamFailure::Preparation);
        assert!(!failure.is_retryable());
        assert!(control.check().is_err());
        assert_eq!(Arc::strong_count(&control), 1);
    }
}

#[test]
fn decoder_preserves_full_chunks_and_the_last_partial_chunk_before_end() {
    let full = DECODED_CHUNK_FRAMES * 2;
    let control = StreamControl::new(false);
    let (sender, receiver) = mpsc::sync_channel(4);
    decode_stream(samples(full + 4), sender, None, control.clone());
    let lengths: Vec<_> = receiver
        .into_iter()
        .map(|message| match message {
            StreamMessage::Samples(samples) => {
                assert!(samples.iter().all(|sample| *sample == 0.25));
                samples.len()
            }
            StreamMessage::End => 0,
            StreamMessage::Error(error) => panic!("unexpected error: {error}"),
        })
        .collect();
    assert_eq!(lengths, vec![full, 4, 0]);
    assert!(control.fully_decoded());
}

#[test]
fn a_duration_truncation_never_marks_the_decode_complete() {
    let control = StreamControl::new(false);
    let (sender, receiver) = mpsc::sync_channel(4);
    decode_stream(
        samples(2),
        sender,
        Some(Duration::from_secs(180)),
        control.clone(),
    );
    assert!(matches!(
        receiver.recv().unwrap(),
        StreamMessage::Samples(_)
    ));
    let error = match receiver.recv().unwrap() {
        StreamMessage::Error(error) => error,
        _ => panic!("expected truncation"),
    };
    assert_eq!(error.kind, StreamFailure::Truncated);
    assert!(!control.fully_decoded());
}

fn take_startup(receiver: &Receiver<StreamMessage>) {
    for _ in 0..STARTUP_BUFFER_CHUNKS {
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
            StreamMessage::Samples(_)
        ));
    }
}

#[test]
fn speculative_decoder_sleeps_at_startup_target_then_promotes_remaining_pcm() {
    let chunk = DECODED_CHUNK_FRAMES * 2;
    let control = StreamControl::new(true);
    let receiver = spawn_decoder(
        samples(chunk * (STARTUP_BUFFER_CHUNKS + 1)),
        None,
        control.clone(),
    )
    .unwrap();
    take_startup(&receiver);
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(!control.fully_decoded());
    control.promote();
    assert!(
        matches!(receiver.recv_timeout(Duration::from_secs(5)).unwrap(), StreamMessage::Samples(samples) if samples.len() == chunk)
    );
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
        StreamMessage::End
    ));
    wait_for(|| Arc::strong_count(&control) == 1);
    assert!(control.fully_decoded());
}

#[test]
fn cancelling_a_sleeping_decoder_closes_delivery_without_a_natural_end_event() {
    let control = StreamControl::new(true);
    let receiver = spawn_decoder(
        samples(DECODED_CHUNK_FRAMES * 2 * (STARTUP_BUFFER_CHUNKS + 1)),
        None,
        control.clone(),
    )
    .unwrap();
    take_startup(&receiver);
    control.cancel();
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    wait_for(|| Arc::strong_count(&control) == 1);
    assert!(!control.fully_decoded());
}
