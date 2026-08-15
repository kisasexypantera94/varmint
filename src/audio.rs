use coreaudio::audio_unit::{
    AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat,
    audio_format::LinearPcmFlags,
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
    },
    thread::{self, JoinHandle},
};
use zerocopy::IntoBytes;

const MAX_PERIOD_BYTES: usize = 16 * 1024;
const RING_CAPACITY: usize = 8;

struct Period {
    seq: u64,
    generation: u64,
    len: usize,
    data: [u8; MAX_PERIOD_BYTES],
}

pub enum BackendEvent {
    PeriodElapsed(u64),
}

pub struct Backend {
    _unit: AudioUnit,
    completion: Arc<CompletionState>,
    completion_thread: Option<JoinHandle<()>>,
}

pub struct PeriodSink {
    producer: Producer<Period>,
    playback: Arc<PlaybackState>,
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
    pending: AtomicBool,
    running: AtomicBool,
}

impl Backend {
    pub fn new<F>(on_event: F) -> Result<(Backend, PeriodSink), coreaudio::Error>
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
            pending: AtomicBool::new(false),
            running: AtomicBool::new(true),
        });

        let completion_state = Arc::clone(&completion);
        let completion_thread = thread::Builder::new()
            .name("audio-completion".into())
            .spawn(move || {
                while completion_state.running.load(Ordering::Acquire) {
                    thread::park();

                    while completion_state.pending.swap(false, Ordering::AcqRel) {
                        let seq = completion_state.latest_seq.load(Ordering::Acquire);
                        on_event(BackendEvent::PeriodElapsed(seq));
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

        Ok((
            Backend {
                _unit: unit,
                completion,
                completion_thread: Some(completion_thread),
            },
            PeriodSink { producer, playback },
        ))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
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
                    self.completion.pending.store(true, Ordering::Release);
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
