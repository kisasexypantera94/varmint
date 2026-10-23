use crate::{
    audio::{CaptureSource, PeriodSink},
    memory::GuestMemory,
    virtio::{
        chain::ChainData,
        common,
        device::{ChainAction, ChainToken, Device, DeviceContext, ExternalEventHandler},
    },
};
use num_enum::TryFromPrimitive;
use std::collections::VecDeque;
use zerocopy::{FromBytes, Immutable, IntoBytes};

const DEVICE_ID: u32 = 25;

const NUM_JACKS: u32 = 0;
const NUM_STREAMS: u32 = 2;
const NUM_CHMAPS: u32 = 2;

const PLAYBACK_STREAM_ID: u32 = 0;
const CAPTURE_STREAM_ID: u32 = 1;

const CHMAP_MAX_SIZE: usize = 18;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
enum RequestCode {
    JackInfo = 0x0001,
    JackRemap,

    PcmInfo = 0x0100,
    PcmSetParams,
    PcmPrepare,
    PcmRelease,
    PcmStart,
    PcmStop,

    ChmapInfo = 0x0200,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
enum Status {
    Ok = 0x8000,
    BadMsg,
    NotSupp,
    IoErr,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
enum EventType {
    JackConnected = 0x1000,
    JackDisconnected,
    PcmPeriodElapsed = 0x1100,
    PcmXrun,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
enum Direction {
    Output = 0,
    Input = 1,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
enum PcmFormat {
    ImaAdpcm = 0,
    MuLaw,
    ALaw,
    S8,
    U8,
    S16,
    U16,
    S18_3,
    U18_3,
    S20_3,
    U20_3,
    S24_3,
    U24_3,
    S20,
    U20,
    S24,
    U24,
    S32,
    U32,
    Float,
    Float64,
    DsdU8,
    DsdU16,
    DsdU32,
    Iec958Subframe,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
enum PcmRate {
    R5512 = 0,
    R8000,
    R11025,
    R16000,
    R22050,
    R32000,
    R44100,
    R48000,
    R64000,
    R88200,
    R96000,
    R176400,
    R192000,
    R384000,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
enum ChmapPosition {
    None = 0,
    Na,
    Mono,
    Fl,
    Fr,
    Rl,
    Rr,
    Fc,
    Lfe,
    Sl,
    Sr,
    Rc,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(usize)]
enum QueueType {
    Control = 0,
    Event = 1,
    Tx = 2,
    Rx = 3,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct Hdr {
    code: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct QueryInfo {
    hdr: Hdr,
    start_id: u32,
    count: u32,
    size: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct Info {
    hda_fn_nid: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct PcmInfo {
    hdr: Info,
    features: u32,
    formats: u64,
    rates: u64,
    direction: u8,
    channels_min: u8,
    channels_max: u8,
    padding: [u8; 5],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct PcmHdr {
    hdr: Hdr,
    stream_id: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct PcmSetParams {
    hdr: PcmHdr,
    buffer_bytes: u32,
    period_bytes: u32,
    features: u32,
    channels: u8,
    format: u8,
    rate: u8,
    padding: u8,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct PcmXfer {
    stream_id: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct PcmStatus {
    status: u32,
    latency_bytes: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct ChmapInfo {
    hdr: Info,
    direction: u8,
    channels: u8,
    positions: [u8; CHMAP_MAX_SIZE],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct JackInfo {
    hdr: Info,
    features: u32,
    hda_reg_defconf: u32,
    hda_reg_caps: u32,
    connected: u8,
    padding: [u8; 7],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct Event {
    hdr: Hdr,
    data: u32,
}

#[repr(C)]
#[derive(Default, IntoBytes, Immutable)]
struct Config {
    jacks: u32,
    streams: u32,
    chmaps: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum StreamState {
    Initial,
    ParamsSet,
    Prepared,
    Running,
}

struct Stream {
    state: StreamState,
    buffer_bytes: u32,
    period_scratch: Vec<u8>,
    channels: u8,
    format: u8,
    rate: u8,
}

impl Stream {
    fn new() -> Stream {
        Stream {
            state: StreamState::Initial,
            buffer_bytes: 0,
            period_scratch: Vec::new(),
            channels: 0,
            format: 0,
            rate: 0,
        }
    }
}

struct PendingPeriod {
    seq: u64,
    token: ChainToken,
    written: u32,
}

struct PendingCapture {
    chain: ChainData,
    token: ChainToken,
}

pub struct Snd {
    period_sink: PeriodSink,
    capture_source: CaptureSource,

    streams: [Stream; NUM_STREAMS as usize],

    pending: VecDeque<PendingPeriod>,
    next_period: u64,
    consumed_through: Option<u64>,
    tx_completions_since_notify: usize,

    capture_pending: VecDeque<PendingCapture>,
    capture_fill: usize,
}

impl Snd {
    pub fn new(period_sink: PeriodSink, capture_source: CaptureSource) -> Snd {
        Snd {
            period_sink,
            capture_source,
            streams: [Stream::new(), Stream::new()],
            pending: VecDeque::new(),
            next_period: 0,
            consumed_through: None,
            tx_completions_since_notify: 0,
            capture_pending: VecDeque::new(),
            capture_fill: 0,
        }
    }
}

impl Device for Snd {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1
    }

    fn num_queues(&self) -> u16 {
        4
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let cfg = Config {
            jacks: NUM_JACKS,
            streams: NUM_STREAMS,
            chmaps: NUM_CHMAPS,
        };

        let offset = offset as usize;
        data.copy_from_slice(&cfg.as_bytes()[offset..offset + data.len()]);
    }

    fn queue_notified(&mut self, queue_idx: usize, ctx: &mut DeviceContext<'_>) {
        match QueueType::try_from(queue_idx).unwrap() {
            QueueType::Control => {
                while let Some(chain) = ctx.pop_chain(queue_idx) {
                    if let ChainAction::Complete(written) = self.control(&chain.data, ctx) {
                        ctx.complete(chain.token, written);
                    }
                }
            }
            QueueType::Tx => {
                let mut added = false;

                while let Some(chain) = ctx.pop_chain(queue_idx) {
                    if !added {
                        self.tx_completions_since_notify = 0;
                        added = true;
                    }

                    if let ChainAction::Complete(written) = self.submit_period(&chain.data, chain.token, ctx.mem()) {
                        ctx.complete(chain.token, written);
                        self.tx_completions_since_notify += 1;
                    }
                }

                if added {
                    self.publish_consumed(ctx);
                }
            }
            QueueType::Rx => {
                while let Some(chain) = ctx.pop_chain(queue_idx) {
                    match self.submit_capture(chain.data, chain.token, ctx.mem()) {
                        ChainAction::Complete(written) => {
                            ctx.complete(chain.token, written);
                        }
                        ChainAction::Deferred => {}
                    }
                }

                self.publish_capture(ctx);
            }
            QueueType::Event => {}
        }
    }

    fn reset(&mut self) {
        self.period_sink.reset();
        self.capture_source.reset();

        self.pending.clear();
        self.capture_pending.clear();

        self.capture_fill = 0;
        self.consumed_through = None;
        self.tx_completions_since_notify = 0;

        self.streams = [Stream::new(), Stream::new()];
    }
}

impl Snd {
    fn control(&mut self, chain: &ChainData, ctx: &mut DeviceContext<'_>) -> ChainAction {
        let Some(hdr) = chain.read_obj::<Hdr>(0, ctx.mem()) else {
            eprintln!("virtio-snd: unreadable control header");
            return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
        };

        match RequestCode::try_from(hdr.code) {
            Ok(RequestCode::PcmInfo) => {
                let Some(q) = chain.read_obj::<QueryInfo>(0, ctx.mem()) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                let (start, count) = (q.start_id, q.count);

                if start + count > NUM_STREAMS {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                }

                let mut resp = Hdr {
                    code: Status::Ok as u32,
                }
                .as_bytes()
                .to_vec();

                for stream_id in start..start + count {
                    let (direction, channels) = match stream_id {
                        PLAYBACK_STREAM_ID => (Direction::Output, 2),
                        CAPTURE_STREAM_ID => (Direction::Input, 1),
                        _ => unreachable!(),
                    };

                    let info = PcmInfo {
                        hdr: Info { hda_fn_nid: 0 },
                        features: 0,
                        formats: 1u64 << PcmFormat::S16 as u64,
                        rates: 1u64 << PcmRate::R48000 as u64,
                        direction: direction as u8,
                        channels_min: channels,
                        channels_max: channels,
                        padding: [0; 5],
                    };

                    resp.extend_from_slice(info.as_bytes());
                }

                ChainAction::Complete(chain.write_response(&resp, ctx.mem()))
            }
            Ok(RequestCode::PcmSetParams) => {
                let Some(p) = chain.read_obj::<PcmSetParams>(0, ctx.mem()) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                let expected_channels = match p.hdr.stream_id {
                    PLAYBACK_STREAM_ID => 2,
                    CAPTURE_STREAM_ID => 1,
                    _ => {
                        return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                    }
                };

                let stream = &mut self.streams[p.hdr.stream_id as usize];

                if matches!(stream.state, StreamState::Running) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                }

                if p.format != PcmFormat::S16 as u8
                    || p.rate != PcmRate::R48000 as u8
                    || p.channels != expected_channels
                    || p.features != 0
                {
                    return ChainAction::Complete(self.respond_status(chain, Status::NotSupp, ctx.mem()));
                }

                let (period, buffer) = (p.period_bytes, p.buffer_bytes);

                let frame_bytes = expected_channels as u32 * std::mem::size_of::<i16>() as u32;

                if period == 0 || buffer % period != 0 || period % frame_bytes != 0 {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                }

                stream.buffer_bytes = buffer;
                stream.period_scratch = vec![0; period as usize];
                stream.channels = p.channels;
                stream.format = p.format;
                stream.rate = p.rate;
                stream.state = StreamState::ParamsSet;

                ChainAction::Complete(self.respond_status(chain, Status::Ok, ctx.mem()))
            }
            Ok(RequestCode::PcmPrepare) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, ctx.mem()) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                let Some(stream) = self.streams.get_mut(p.stream_id as usize) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                if matches!(stream.state, StreamState::Initial | StreamState::Running) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                }

                stream.state = StreamState::Prepared;
                ChainAction::Complete(self.respond_status(chain, Status::Ok, ctx.mem()))
            }
            Ok(RequestCode::PcmStart) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, ctx.mem()) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                let stream_id = p.stream_id as usize;

                let Some(stream) = self.streams.get(stream_id) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                if !matches!(stream.state, StreamState::Prepared) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                }

                match p.stream_id {
                    PLAYBACK_STREAM_ID => {
                        self.period_sink.start();
                    }
                    CAPTURE_STREAM_ID => {
                        if !self.capture_source.start() {
                            return ChainAction::Complete(self.respond_status(chain, Status::IoErr, ctx.mem()));
                        }
                    }
                    _ => unreachable!(),
                }

                self.streams[stream_id].state = StreamState::Running;

                ChainAction::Complete(self.respond_status(chain, Status::Ok, ctx.mem()))
            }
            Ok(RequestCode::PcmStop) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, ctx.mem()) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                let stream_id = p.stream_id as usize;

                let Some(stream) = self.streams.get(stream_id) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                if !matches!(stream.state, StreamState::Running) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                }

                match p.stream_id {
                    PLAYBACK_STREAM_ID => {
                        self.period_sink.stop();
                    }
                    CAPTURE_STREAM_ID => {
                        self.capture_source.stop();
                    }
                    _ => unreachable!(),
                }

                self.streams[stream_id].state = StreamState::Prepared;

                ChainAction::Complete(self.respond_status(chain, Status::Ok, ctx.mem()))
            }
            Ok(RequestCode::PcmRelease) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, ctx.mem()) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                let Some(stream) = self.streams.get(p.stream_id as usize) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                if !matches!(stream.state, StreamState::ParamsSet | StreamState::Prepared) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                }

                match p.stream_id {
                    PLAYBACK_STREAM_ID => {
                        self.period_sink.reset();
                        self.consumed_through = None;
                        self.tx_completions_since_notify = 0;

                        while let Some(period) = self.pending.pop_front() {
                            ctx.complete(period.token, period.written);
                        }

                        while let Some(period) = ctx.pop_chain(QueueType::Tx as usize) {
                            let written = self.respond_pcm_status(&period.data, Status::IoErr, ctx.mem());

                            ctx.complete(period.token, written);
                        }
                    }

                    CAPTURE_STREAM_ID => {
                        self.capture_source.reset();

                        if let Some(pending) = self.capture_pending.pop_front() {
                            let written = {
                                let pcm = &self.streams[CAPTURE_STREAM_ID as usize].period_scratch[..self.capture_fill];

                                Self::write_capture_response(&pending.chain, pcm, Status::Ok, ctx.mem())
                            };

                            ctx.complete(pending.token, written);
                        }

                        self.capture_fill = 0;

                        while let Some(pending) = self.capture_pending.pop_front() {
                            let written = Self::write_capture_response(&pending.chain, &[], Status::Ok, ctx.mem());

                            ctx.complete(pending.token, written);
                        }

                        while let Some(pending) = ctx.pop_chain(QueueType::Rx as usize) {
                            let written = Self::write_capture_response(&pending.data, &[], Status::Ok, ctx.mem());

                            ctx.complete(pending.token, written);
                        }
                    }

                    _ => unreachable!(),
                }

                self.streams[p.stream_id as usize] = Stream::new();

                ChainAction::Complete(self.respond_status(chain, Status::Ok, ctx.mem()))
            }
            Ok(RequestCode::ChmapInfo) => {
                let Some(q) = chain.read_obj::<QueryInfo>(0, ctx.mem()) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                };

                let (start, count) = (q.start_id, q.count);

                match start.checked_add(count) {
                    Some(end) if end <= NUM_CHMAPS => {}
                    _ => {
                        return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()));
                    }
                }

                let mut resp = Hdr {
                    code: Status::Ok as u32,
                }
                .as_bytes()
                .to_vec();

                for stream_id in start..start + count {
                    let mut positions = [ChmapPosition::None as u8; CHMAP_MAX_SIZE];

                    let (direction, channels) = match stream_id {
                        PLAYBACK_STREAM_ID => {
                            positions[0] = ChmapPosition::Fl as u8;

                            positions[1] = ChmapPosition::Fr as u8;

                            (Direction::Output, 2)
                        }

                        CAPTURE_STREAM_ID => {
                            positions[0] = ChmapPosition::Mono as u8;

                            (Direction::Input, 1)
                        }

                        _ => unreachable!(),
                    };

                    let info = ChmapInfo {
                        hdr: Info { hda_fn_nid: 0 },
                        direction: direction as u8,
                        channels,
                        positions,
                    };

                    resp.extend_from_slice(info.as_bytes());
                }

                ChainAction::Complete(chain.write_response(&resp, ctx.mem()))
            }
            Ok(RequestCode::JackInfo | RequestCode::JackRemap) => {
                ChainAction::Complete(self.respond_status(chain, Status::NotSupp, ctx.mem()))
            }
            Err(v) => {
                eprintln!("virtio-snd: unknown control code: 0x{:x}", v.number);
                ChainAction::Complete(self.respond_status(chain, Status::BadMsg, ctx.mem()))
            }
        }
    }

    fn submit_period(&mut self, chain: &ChainData, token: ChainToken, mem: &GuestMemory) -> ChainAction {
        const PAYLOAD_OFFSET: usize = size_of::<PcmXfer>();

        let Some(pcm_xfer) = chain.read_obj::<PcmXfer>(0, mem) else {
            return ChainAction::Complete(0);
        };

        let stream = &mut self.streams[PLAYBACK_STREAM_ID as usize];

        if pcm_xfer.stream_id != PLAYBACK_STREAM_ID
            || !matches!(stream.state, StreamState::Prepared | StreamState::Running)
        {
            return ChainAction::Complete(self.respond_pcm_status(chain, Status::IoErr, mem));
        }

        if chain.read_at(PAYLOAD_OFFSET, &mut stream.period_scratch, mem).is_none() {
            return ChainAction::Complete(0);
        }

        if !self.period_sink.push(self.next_period, &stream.period_scratch) {
            return ChainAction::Complete(self.respond_pcm_status(chain, Status::IoErr, mem));
        }

        let written = self.respond_pcm_status(chain, Status::Ok, mem);

        self.pending.push_back(PendingPeriod {
            seq: self.next_period,
            token,
            written,
        });

        self.next_period = self.next_period.wrapping_add(1);

        ChainAction::Deferred
    }

    fn publish_consumed(&mut self, ctx: &mut DeviceContext<'_>) {
        let Some(consumed_through) = self.consumed_through else {
            return;
        };

        let stream = &self.streams[PLAYBACK_STREAM_ID as usize];

        let period_bytes = stream.period_scratch.len();

        let periods = if period_bytes == 0 {
            1
        } else {
            stream.buffer_bytes as usize / period_bytes
        };

        // virtio-snd driver can miss a full buffer wrap when completions are batched.
        // Keep one period pending until the guest refills the TX queue.
        let max_without_refill = periods.saturating_sub(1).max(1);

        while self.tx_completions_since_notify < max_without_refill {
            let Some(front) = self.pending.front() else {
                break;
            };
            if front.seq > consumed_through {
                break;
            }

            let period = self.pending.pop_front().unwrap();
            ctx.complete(period.token, period.written);
            self.tx_completions_since_notify += 1;
        }
    }

    fn submit_capture(&mut self, chain: ChainData, token: ChainToken, mem: &GuestMemory) -> ChainAction {
        let Some(pcm_xfer) = chain.read_obj::<PcmXfer>(0, mem) else {
            return ChainAction::Complete(0);
        };

        let stream = &self.streams[CAPTURE_STREAM_ID as usize];

        if pcm_xfer.stream_id != CAPTURE_STREAM_ID
            || !matches!(stream.state, StreamState::Prepared | StreamState::Running)
        {
            return ChainAction::Complete(0);
        }

        let required = stream.period_scratch.len() + size_of::<PcmStatus>();

        let writable: usize = chain.writable.iter().map(|seg| seg.len as usize).sum();

        if stream.period_scratch.is_empty() || writable < required {
            return ChainAction::Complete(0);
        }

        self.capture_pending.push_back(PendingCapture { chain, token });

        ChainAction::Deferred
    }

    fn publish_capture(&mut self, ctx: &mut DeviceContext<'_>) {
        if !matches!(self.streams[CAPTURE_STREAM_ID as usize].state, StreamState::Running) {
            return;
        }

        loop {
            if self.capture_pending.is_empty() {
                break;
            }

            let period_len = self.streams[CAPTURE_STREAM_ID as usize].period_scratch.len();

            if period_len == 0 {
                break;
            }

            let read = {
                let stream = &mut self.streams[CAPTURE_STREAM_ID as usize];

                self.capture_source
                    .read(&mut stream.period_scratch[self.capture_fill..])
            };

            self.capture_fill += read;

            if self.capture_fill < period_len {
                break;
            }

            let pending = self.capture_pending.pop_front().unwrap();

            let written = {
                let stream = &self.streams[CAPTURE_STREAM_ID as usize];

                Self::write_capture_response(&pending.chain, &stream.period_scratch, Status::Ok, ctx.mem())
            };

            ctx.complete(pending.token, written);

            self.capture_fill = 0;
        }
    }

    fn write_capture_response(chain: &ChainData, pcm: &[u8], status: Status, mem: &GuestMemory) -> u32 {
        let status = PcmStatus {
            status: status as u32,
            latency_bytes: 0,
        };

        chain.write_parts(&[pcm, status.as_bytes()], mem)
    }

    fn respond_status(&self, chain: &ChainData, status: Status, mem: &GuestMemory) -> u32 {
        let hdr = Hdr { code: status as u32 };
        chain.write_response(hdr.as_bytes(), mem)
    }

    fn respond_pcm_status(&self, chain: &ChainData, status: Status, mem: &GuestMemory) -> u32 {
        let status = PcmStatus {
            status: status as u32,
            latency_bytes: 0,
        };
        chain.write_response(status.as_bytes(), mem)
    }
}

pub enum ExternalEvent {
    PeriodElapsed(u64),
    CaptureReady,
}

impl ExternalEventHandler for Snd {
    type Event<'a> = ExternalEvent;

    fn on_event(&mut self, event: ExternalEvent, ctx: &mut DeviceContext<'_>) {
        match event {
            ExternalEvent::PeriodElapsed(seq) => {
                self.consumed_through = Some(seq);
                self.publish_consumed(ctx);
            }
            ExternalEvent::CaptureReady => {
                self.publish_capture(ctx);
            }
        }
    }
}
