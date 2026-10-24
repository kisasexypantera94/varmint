use coreaudio::audio_unit::{
    AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat,
    audio_format::LinearPcmFlags,
    macos_helpers::{audio_unit_from_device_id_uninitialized, get_default_device_id},
    render_callback::{
        self, Args,
        data::{self, Interleaved},
    },
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
};
use zerocopy::IntoBytes;

const MAX_PERIOD_BYTES: usize = 16 * 1024;
const RING_CAPACITY: usize = 8;
const CAPTURE_RING_CAPACITY: usize = 16;

struct Period {
    seq: u64,
    generation: u64,
    len: usize,
    data: [u8; MAX_PERIOD_BYTES],
}

struct CaptureChunk {
    len: usize,
    data: [u8; MAX_PERIOD_BYTES],
}

enum CaptureCommand {
    Start(Sender<bool>),
    Stop(Sender<()>),
    Shutdown,
}

pub enum BackendEvent {
    PeriodElapsed(u64),
    CaptureReady,
}

pub struct Backend {
    unit: AudioUnit,
    completion: Arc<CompletionState>,
    completion_thread: Option<JoinHandle<()>>,
    capture_control: Sender<CaptureCommand>,
    capture_thread: Option<JoinHandle<()>>,
}

pub struct PeriodSink {
    producer: Producer<Period>,
    playback: Arc<PlaybackState>,
}

pub struct CaptureSource {
    consumer: Consumer<CaptureChunk>,
    control: Sender<CaptureCommand>,
    running: Arc<AtomicBool>,
    current: Option<(CaptureChunk, usize)>,
}

struct CallbackState {
    consumer: Consumer<Period>,
    completion: Arc<CompletionState>,
    completion_thread: thread::Thread,
    playback: Arc<PlaybackState>,
    current: Option<(Period, usize)>,
}

struct PlaybackState {
    running: AtomicBool,
    generation: AtomicU64,
}

struct CompletionState {
    latest_seq: AtomicU64,
    playback_pending: AtomicBool,
    capture_pending: AtomicBool,
    running: AtomicBool,
}

impl Backend {
    pub fn new<F>(on_event: F) -> Result<(Backend, PeriodSink, CaptureSource), coreaudio::Error>
    where
        F: Fn(BackendEvent) + Send + 'static,
    {
        let (producer, consumer) = RingBuffer::new(RING_CAPACITY);

        let playback = Arc::new(PlaybackState {
            running: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        });

        let completion = Arc::new(CompletionState {
            latest_seq: AtomicU64::new(0),
            playback_pending: AtomicBool::new(false),
            capture_pending: AtomicBool::new(false),
            running: AtomicBool::new(true),
        });

        let completion_state = Arc::clone(&completion);
        let completion_thread = thread::Builder::new()
            .name("audio-completion".into())
            .spawn(move || {
                while completion_state.running.load(Ordering::Acquire) {
                    thread::park();

                    if !completion_state.running.load(Ordering::Acquire) {
                        break;
                    }

                    while completion_state.playback_pending.swap(false, Ordering::AcqRel) {
                        let seq = completion_state.latest_seq.load(Ordering::Acquire);
                        on_event(BackendEvent::PeriodElapsed(seq));
                    }

                    while completion_state.capture_pending.swap(false, Ordering::AcqRel) {
                        on_event(BackendEvent::CaptureReady);
                    }
                }
            })
            .expect("failed to start audio completion thread");

        let mut unit = AudioUnit::new(IOType::DefaultOutput)?;

        let stream_format = StreamFormat {
            sample_rate: 48000.0,
            sample_format: SampleFormat::I16,
            flags: LinearPcmFlags::IS_PACKED | LinearPcmFlags::IS_SIGNED_INTEGER,
            channels: 2,
        };

        unit.set_stream_format(stream_format, Scope::Input, Element::Output)?;

        let mut state = CallbackState {
            consumer,
            completion: Arc::clone(&completion),
            completion_thread: completion_thread.thread().clone(),
            playback: Arc::clone(&playback),
            current: None,
        };

        unit.set_render_callback(move |args: render_callback::Args<data::Interleaved<i16>>| {
            state.render(args);
            Ok(())
        })?;

        unit.start()?;

        let (capture_producer, capture_consumer) = RingBuffer::new(CAPTURE_RING_CAPACITY);

        let capture_running = Arc::new(AtomicBool::new(false));
        let (capture_control, capture_control_rx) = mpsc::channel();

        let capture_completion = Arc::clone(&completion);
        let capture_completion_thread = completion_thread.thread().clone();
        let capture_callback_running = Arc::clone(&capture_running);

        let capture_thread = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                let mut producer = Some(capture_producer);
                let mut unit: Option<AudioUnit> = None;

                while let Ok(command) = capture_control_rx.recv() {
                    match command {
                        CaptureCommand::Start(reply) => {
                            if unit.is_none() {
                                let Some(device_id) = get_default_device_id(true) else {
                                    eprintln!("audio: no default microphone is available");
                                    let _ = reply.send(false);
                                    continue;
                                };

                                let mut new_unit = match audio_unit_from_device_id_uninitialized(device_id, true) {
                                    Ok(unit) => unit,
                                    Err(error) => {
                                        eprintln!("audio: failed to create microphone input: {error}");
                                        let _ = reply.send(false);
                                        break;
                                    }
                                };

                                let stream_format = StreamFormat {
                                    sample_rate: 48000.0,
                                    sample_format: SampleFormat::I16,
                                    flags: LinearPcmFlags::IS_PACKED
                                        | LinearPcmFlags::IS_SIGNED_INTEGER
                                        | LinearPcmFlags::IS_NON_INTERLEAVED,
                                    channels: 1,
                                };

                                if let Err(error) =
                                    new_unit.set_stream_format(stream_format, Scope::Output, Element::Input)
                                {
                                    eprintln!("audio: failed to configure microphone input: {error}");
                                    let _ = reply.send(false);
                                    break;
                                }

                                let mut callback_producer =
                                    producer.take().expect("microphone producer already consumed");

                                let callback_completion = Arc::clone(&capture_completion);

                                let callback_completion_thread = capture_completion_thread.clone();

                                let callback_running = Arc::clone(&capture_callback_running);

                                if let Err(error) =
                                    new_unit.set_input_callback(move |args: Args<data::NonInterleaved<i16>>| {
                                        if !callback_running.load(Ordering::Acquire) {
                                            return Ok(());
                                        }

                                        let Some(channel) = args.data.channels().next() else {
                                            return Ok(());
                                        };

                                        let samples_per_chunk = MAX_PERIOD_BYTES / std::mem::size_of::<i16>();

                                        let mut pushed = false;

                                        for samples in channel.chunks(samples_per_chunk) {
                                            let mut chunk = CaptureChunk {
                                                len: samples.len() * std::mem::size_of::<i16>(),
                                                data: [0; MAX_PERIOD_BYTES],
                                            };

                                            for (dst, sample) in
                                                chunk.data[..chunk.len].chunks_exact_mut(2).zip(samples.iter())
                                            {
                                                dst.copy_from_slice(&sample.to_le_bytes());
                                            }

                                            if callback_producer.push(chunk).is_err() {
                                                break;
                                            }

                                            pushed = true;
                                        }

                                        if pushed {
                                            callback_completion.capture_pending.store(true, Ordering::Release);

                                            callback_completion_thread.unpark();
                                        }

                                        Ok(())
                                    })
                                {
                                    eprintln!("audio: failed to install microphone callback: {error}");
                                    let _ = reply.send(false);
                                    break;
                                }

                                if let Err(error) = new_unit.initialize() {
                                    eprintln!("audio: failed to initialize microphone input: {error}");
                                    let _ = reply.send(false);
                                    break;
                                }

                                unit = Some(new_unit);
                            }

                            capture_callback_running.store(true, Ordering::Release);

                            let started = if let Some(unit) = unit.as_mut() {
                                if let Err(error) = unit.start() {
                                    eprintln!("audio: failed to start microphone input: {error}");
                                    capture_callback_running.store(false, Ordering::Release);
                                    false
                                } else {
                                    true
                                }
                            } else {
                                false
                            };

                            let _ = reply.send(started);
                        }

                        CaptureCommand::Stop(reply) => {
                            capture_callback_running.store(false, Ordering::Release);

                            if let Some(unit) = unit.as_mut() {
                                if let Err(error) = unit.stop() {
                                    eprintln!("audio: failed to stop microphone input: {error}");
                                }
                            }

                            let _ = reply.send(());
                        }

                        CaptureCommand::Shutdown => {
                            capture_callback_running.store(false, Ordering::Release);

                            if let Some(unit) = unit.as_mut() {
                                let _ = unit.stop();
                            }
                            break;
                        }
                    }
                }
            })
            .expect("failed to start audio capture thread");

        Ok((
            Backend {
                unit,
                completion,
                completion_thread: Some(completion_thread),
                capture_control: capture_control.clone(),
                capture_thread: Some(capture_thread),
            },
            PeriodSink { producer, playback },
            CaptureSource {
                consumer: capture_consumer,
                control: capture_control,
                running: capture_running,
                current: None,
            },
        ))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.unit.stop();

        let _ = self.capture_control.send(CaptureCommand::Shutdown);

        if let Some(thread) = self.capture_thread.take() {
            let _ = thread.join();
        }

        self.completion.running.store(false, Ordering::Release);

        if let Some(thread) = self.completion_thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

impl CallbackState {
    fn render(&mut self, args: Args<Interleaved<i16>>) {
        let out = args.data.buffer.as_mut_bytes();
        let mut filled = 0;

        if !self.playback.running.load(Ordering::Acquire) {
            out.fill(0);
            return;
        }

        while filled < out.len() && self.playback.running.load(Ordering::Acquire) {
            let generation = self.playback.generation.load(Ordering::Acquire);

            if self
                .current
                .as_ref()
                .is_some_and(|(period, _)| period.generation != generation)
            {
                self.current = None;
            }

            if self.current.is_none() {
                loop {
                    match self.consumer.pop() {
                        Ok(period) if period.generation == generation => {
                            self.current = Some((period, 0));
                            break;
                        }
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }

                if self.current.is_none() {
                    break;
                }
            }

            let (period, off) = self.current.as_mut().unwrap();

            let n = (period.len - *off).min(out.len() - filled);

            out[filled..filled + n].copy_from_slice(&period.data[*off..*off + n]);

            filled += n;
            *off += n;

            if *off == period.len {
                let (done, _) = self.current.take().unwrap();

                if done.generation == self.playback.generation.load(Ordering::Acquire) {
                    self.completion.latest_seq.store(done.seq, Ordering::Release);

                    self.completion.playback_pending.store(true, Ordering::Release);

                    self.completion_thread.unpark();
                }
            }
        }

        out[filled..].fill(0);
    }
}

impl PeriodSink {
    pub fn push(&mut self, seq: u64, bytes: &[u8]) -> bool {
        if bytes.len() > MAX_PERIOD_BYTES {
            return false;
        }

        let mut period = Period {
            seq,
            generation: self.playback.generation.load(Ordering::Acquire),
            len: bytes.len(),
            data: [0; MAX_PERIOD_BYTES],
        };

        period.data[..bytes.len()].copy_from_slice(bytes);

        self.producer.push(period).is_ok()
    }

    pub fn start(&self) {
        self.playback.running.store(true, Ordering::Release);
    }

    pub fn stop(&self) {
        self.playback.running.store(false, Ordering::Release);
    }

    pub fn reset(&self) {
        self.playback.running.store(false, Ordering::Release);

        self.playback.generation.fetch_add(1, Ordering::AcqRel);
    }
}

impl CaptureSource {
    pub fn start(&mut self) -> bool {
        self.running.store(false, Ordering::Release);

        self.clear();

        let (reply_tx, reply_rx) = mpsc::channel();

        if self.control.send(CaptureCommand::Start(reply_tx)).is_err() {
            return false;
        }

        reply_rx.recv().unwrap_or(false)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);

        let (reply_tx, reply_rx) = mpsc::channel();

        if self.control.send(CaptureCommand::Stop(reply_tx)).is_ok() {
            let _ = reply_rx.recv();
        }

        self.clear();
    }

    pub fn reset(&mut self) {
        self.stop();
    }

    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let mut filled = 0;

        while filled < out.len() {
            if self.current.is_none() {
                let Ok(chunk) = self.consumer.pop() else {
                    break;
                };

                self.current = Some((chunk, 0));
            }

            let done = {
                let (chunk, off) = self.current.as_mut().unwrap();

                let n = (chunk.len - *off).min(out.len() - filled);

                out[filled..filled + n].copy_from_slice(&chunk.data[*off..*off + n]);

                filled += n;
                *off += n;

                *off == chunk.len
            };

            if done {
                self.current = None;
            }
        }

        filled
    }

    fn clear(&mut self) {
        self.current = None;

        while self.consumer.pop().is_ok() {}
    }
}
