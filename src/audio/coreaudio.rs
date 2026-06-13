use crate::kick;
use coreaudio::audio_unit::{
    AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat,
    audio_format::LinearPcmFlags,
    render_callback::{
        self, Args,
        data::{self, Interleaved},
    },
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::mpsc::{self, Receiver, Sender};
use zerocopy::IntoBytes;

const MAX_PERIOD_BYTES: usize = 16 * 1024;
const RING_CAPACITY: usize = 8;

struct Period {
    seq: u64,
    len: usize,
    data: [u8; MAX_PERIOD_BYTES],
}

pub enum BackendEvent {
    PeriodElapsed(u64),
}

pub struct Backend {
    _unit: AudioUnit,
}

pub struct PeriodSink {
    producer: Producer<Period>,
}

struct CallbackState {
    consumer: Consumer<Period>,
    tx: Sender<BackendEvent>,
    kicker: kick::Kicker,
    current: Option<(Period, usize)>,
}

impl Backend {
    pub fn new(
        kicker: kick::Kicker,
    ) -> Result<(Backend, PeriodSink, Receiver<BackendEvent>), coreaudio::Error> {
        let (producer, consumer) = RingBuffer::new(RING_CAPACITY);
        let (tx, rx) = mpsc::channel();

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
            tx,
            kicker,
            current: None,
        };

        unit.set_render_callback(move |args: render_callback::Args<data::Interleaved<i16>>| {
            state.render(args);
            Ok(())
        })?;

        unit.start()?;

        Ok((Backend { _unit: unit }, PeriodSink { producer }, rx))
    }
}

impl CallbackState {
    fn render(&mut self, args: Args<Interleaved<i16>>) {
        let out = args.data.buffer.as_mut_bytes();

        let mut filled = 0;

        while filled < out.len() {
            if self.current.is_none() {
                match self.consumer.pop() {
                    Ok(p) => self.current = Some((p, 0)),
                    Err(_) => break,
                }
            }

            let (period, off) = self.current.as_mut().unwrap();
            let n = (period.len - *off).min(out.len() - filled);
            out[filled..filled + n].copy_from_slice(&period.data[*off..*off + n]);
            filled += n;
            *off += n;

            if *off == period.len {
                let (done, _) = self.current.take().unwrap();
                let _ = self.tx.send(BackendEvent::PeriodElapsed(done.seq));
                self.kicker.kick();
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
            len: bytes.len(),
            data: [0; MAX_PERIOD_BYTES],
        };
        period.data[..bytes.len()].copy_from_slice(bytes);
        self.producer.push(period).is_ok()
    }
}
