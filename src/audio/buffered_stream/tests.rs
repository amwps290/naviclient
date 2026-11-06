use super::super::error::classify_anyhow;
use super::*;

pub(in crate::audio) fn from_samples(
    samples: Vec<Sample>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    duration: Option<Duration>,
) -> (BufferedStreamSource, SharedStreamError) {
    let (_, receiver) = mpsc::channel();
    let error = Arc::new(Mutex::new(None));
    let source = BufferedStreamSource {
        control: StreamControl::new(true),
        receiver,
        queued: VecDeque::from([samples]),
        current: Vec::new().into_iter(),
        channels,
        sample_rate,
        duration,
        producer_finished: true,
        terminal_error: None,
        shared_error: error.clone(),
        last_data: Instant::now(),
        silence_remaining: 0,
    };
    (source, error)
}

pub(in crate::audio) fn assert_sleeping_startup(source: &BufferedStreamSource) {
    assert_eq!(source.queued.len(), STARTUP_BUFFER_CHUNKS);
    assert!(matches!(
        source.receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn temporary_decoder_starvation_outputs_silence_instead_of_ending() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let shared_error = Arc::new(Mutex::new(None));
    let mut source = BufferedStreamSource {
        control: StreamControl::new(false),
        receiver,
        queued: VecDeque::new(),
        current: Vec::new().into_iter(),
        channels: ChannelCount::new(2).unwrap(),
        sample_rate: SampleRate::new(48_000).unwrap(),
        duration: Some(Duration::from_secs(180)),
        producer_finished: false,
        terminal_error: None,
        shared_error: Arc::clone(&shared_error),
        last_data: Instant::now(),
        silence_remaining: 0,
    };

    assert_eq!(source.next(), Some(0.0));
    assert_eq!(source.next(), Some(0.0));
    assert!(shared_error.lock().unwrap().is_none());
    drop(sender);
}

fn waiting_source(
    control: Arc<StreamControl>,
) -> (mpsc::SyncSender<StreamMessage>, BufferedStreamSource) {
    let (sender, receiver) = mpsc::sync_channel(4);
    let source = BufferedStreamSource {
        control,
        receiver,
        queued: VecDeque::new(),
        current: Vec::new().into_iter(),
        channels: ChannelCount::new(2).unwrap(),
        sample_rate: SampleRate::new(48_000).unwrap(),
        duration: None,
        producer_finished: false,
        terminal_error: None,
        shared_error: Arc::new(Mutex::new(None)),
        last_data: Instant::now(),
        silence_remaining: 0,
    };
    (sender, source)
}

#[test]
fn a_normal_end_message_preserves_the_underlying_reader_failure() {
    let control = StreamControl::new(false);
    control.record_io_error(FailureError::new(
        StreamFailure::Truncated,
        "reader range failed",
    ));
    let (sender, source) = waiting_source(control);
    sender.send(StreamMessage::Samples(vec![0.25; 2])).unwrap();
    sender.send(StreamMessage::End).unwrap();
    let error = source.shared_error.clone();
    assert_eq!(source.collect::<Vec<_>>(), vec![0.25; 2]);
    assert_eq!(
        error.lock().unwrap().as_ref().unwrap().kind,
        StreamFailure::Truncated
    );
}

#[test]
fn decoder_disconnect_preserves_the_underlying_reader_failure() {
    let control = StreamControl::new(false);
    control.record_io_error(FailureError::new(
        StreamFailure::Timeout,
        "reader timed out",
    ));
    let (sender, mut source) = waiting_source(control);
    drop(sender);
    assert_eq!(source.next(), None);
    assert_eq!(source.current_span_len(), Some(0));
    assert_eq!(
        source.shared_error.lock().unwrap().as_ref().unwrap().kind,
        StreamFailure::Timeout
    );
}

fn prepare(
    receiver: Receiver<StreamMessage>,
    control: Arc<StreamControl>,
    timeout: Duration,
) -> Result<(BufferedStreamSource, SharedStreamError)> {
    BufferedStreamSource::with_timeout(
        receiver,
        None,
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
        control,
        timeout,
    )
}

fn startup_error(result: Result<(BufferedStreamSource, SharedStreamError)>) -> anyhow::Error {
    match result {
        Ok(_) => panic!("unexpected startup success"),
        Err(error) => error,
    }
}

#[test]
fn startup_collects_only_the_target_then_preserves_remaining_pcm_order() {
    let (sender, receiver) = mpsc::channel();
    let expected: Vec<_> = (0..=STARTUP_BUFFER_CHUNKS)
        .flat_map(|index| [index as f32; 2])
        .collect();
    for samples in expected.chunks(2) {
        sender
            .send(StreamMessage::Samples(samples.to_vec()))
            .unwrap();
    }
    sender.send(StreamMessage::End).unwrap();
    let (source, error) =
        prepare(receiver, StreamControl::new(false), Duration::from_secs(1)).unwrap();
    assert_eq!(source.queued.len(), STARTUP_BUFFER_CHUNKS);
    assert!(!source.producer_finished);
    assert_eq!(source.collect::<Vec<_>>(), expected);
    assert!(error.lock().unwrap().is_none());
}

#[test]
fn partial_startup_samples_are_drained_before_a_terminal_error_is_published() {
    let (sender, receiver) = mpsc::channel();
    sender.send(StreamMessage::Samples(vec![0.25; 4])).unwrap();
    sender
        .send(StreamMessage::Error(FailureError::new(
            StreamFailure::Truncated,
            "early end",
        )))
        .unwrap();
    let (source, error) =
        prepare(receiver, StreamControl::new(false), Duration::from_secs(1)).unwrap();
    assert_eq!(
        source.startup_error().unwrap().kind,
        StreamFailure::Truncated
    );
    assert!(error.lock().unwrap().is_none());
    assert_eq!(source.collect::<Vec<_>>(), vec![0.25; 4]);
    assert_eq!(
        error.lock().unwrap().as_ref().unwrap().kind,
        StreamFailure::Truncated
    );
}

#[test]
fn startup_timeout_cancels_the_decoder_and_closes_its_receiver() {
    let (sender, receiver) = mpsc::channel();
    let control = StreamControl::new(true);
    let error = startup_error(prepare(receiver, control.clone(), Duration::ZERO));
    assert_eq!(classify_anyhow(&error), StreamFailure::NoData);
    assert!(control.check().is_err());
    assert!(sender.send(StreamMessage::End).is_err());
}

#[test]
fn empty_end_failure_and_disconnection_cancel_startup_ownership() {
    for message in [
        Some(StreamMessage::End),
        Some(StreamMessage::Error(FailureError::new(
            StreamFailure::Truncated,
            "early end",
        ))),
        None,
    ] {
        let expected = if matches!(message, Some(StreamMessage::Error(_))) {
            StreamFailure::Truncated
        } else {
            StreamFailure::Decode
        };
        let (sender, receiver) = mpsc::channel();
        if let Some(message) = message {
            sender.send(message).unwrap();
        }
        drop(sender);
        let control = StreamControl::new(false);
        let error = startup_error(prepare(receiver, control.clone(), Duration::from_secs(1)));
        assert_eq!(classify_anyhow(&error), expected);
        assert!(control.check().is_err());
    }
}

#[test]
fn startup_failure_prefers_the_earlier_reader_reason() {
    let (sender, receiver) = mpsc::channel();
    sender
        .send(StreamMessage::Error(FailureError::new(
            StreamFailure::Decode,
            "decoder failed",
        )))
        .unwrap();
    let control = StreamControl::new(false);
    control.record_io_error(FailureError::new(
        StreamFailure::Http(503),
        "server unavailable",
    ));
    let error = startup_error(prepare(receiver, control.clone(), Duration::from_secs(1)));
    assert_eq!(classify_anyhow(&error), StreamFailure::Http(503));
    assert!(error.to_string().contains("server unavailable"));
    assert!(control.check().is_err());
}

#[test]
fn a_short_startup_buffer_remains_usable_when_the_target_wait_expires() {
    let (sender, receiver) = mpsc::channel();
    sender.send(StreamMessage::Samples(vec![0.25; 2])).unwrap();
    let control = StreamControl::new(false);
    let (mut source, error) =
        prepare(receiver, control.clone(), Duration::from_millis(20)).unwrap();
    assert!(control.check().is_ok());
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), Some(0.0));
    assert!(error.lock().unwrap().is_none());
    drop(sender);
}

#[test]
fn a_silence_frame_finishes_all_channels_before_new_pcm_is_delivered() {
    let control = StreamControl::new(false);
    let (sender, mut source) = waiting_source(control.clone());
    source.channels = ChannelCount::new(3).unwrap();
    source.sample_rate = SampleRate::new(1000).unwrap();
    assert_eq!(source.next(), Some(0.0));
    sender
        .send(StreamMessage::Samples(vec![1.0, 2.0, 3.0]))
        .unwrap();
    assert_eq!(source.next(), Some(0.0));
    assert_eq!(source.next(), Some(0.0));
    assert_eq!(
        control.synthetic_silence_duration(),
        Duration::from_millis(1)
    );
    assert_eq!(
        source.by_ref().take(3).collect::<Vec<_>>(),
        vec![1.0, 2.0, 3.0]
    );
    assert!(source.shared_error.lock().unwrap().is_none());
}

#[test]
fn newly_arrived_pcm_resets_stall_timing_before_a_later_empty_poll() {
    let (_sender, mut source) = waiting_source(StreamControl::new(false));
    source.last_data = Instant::now() - STREAM_STALL_TIMEOUT;
    source.queued.push_back(vec![0.25; 2]);
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), Some(0.0));
    assert!(source.shared_error.lock().unwrap().is_none());
}

#[test]
fn permanent_stall_ends_once_and_keeps_the_first_published_error() {
    let (_sender, mut source) = waiting_source(StreamControl::new(false));
    source.last_data = Instant::now() - STREAM_STALL_TIMEOUT;
    assert_eq!(source.next(), None);
    assert_eq!(
        source.shared_error.lock().unwrap().as_ref().unwrap().kind,
        StreamFailure::NoData
    );
    source
        .control
        .record_io_error(FailureError::new(StreamFailure::Truncated, "late failure"));
    assert_eq!(source.next(), None);
    assert_eq!(
        source.shared_error.lock().unwrap().as_ref().unwrap().kind,
        StreamFailure::NoData
    );
}

#[test]
fn promotion_restarts_the_consumers_stall_timing() {
    let (_sender, mut source) = waiting_source(StreamControl::new(true));
    source.last_data = Instant::now() - STREAM_STALL_TIMEOUT;
    source.promote();
    assert_eq!(source.next(), Some(0.0));
    assert!(source.shared_error.lock().unwrap().is_none());
}
