use crate::{
    audio::PeriodSink,
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
const NUM_STREAMS: u32 = 1;
const NUM_CHMAPS: u32 = 1;

const STREAM_ID: u32 = 0;

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

struct PendingRelease {
    token: ChainToken,
    written: u32,
}

pub struct Snd {
    period_sink: PeriodSink,

    stream: Stream,
    pending: VecDeque<PendingPeriod>,
    pending_release: Option<PendingRelease>,
    next_period: u64,
}

impl Snd {
    pub fn new(period_sink: PeriodSink) -> Snd {
        Snd {
            period_sink,
            stream: Stream::new(),
            pending: VecDeque::new(),
            pending_release: None,
            next_period: 0,
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
            QueueType::Control | QueueType::Tx => {
                while let Some(chain) = ctx.pop_chain(queue_idx) {
                    let action = match QueueType::try_from(queue_idx).unwrap() {
                        QueueType::Control => self.control(&chain.data, chain.token, ctx.mem()),
                        QueueType::Tx => self.submit_period(&chain.data, chain.token, ctx.mem()),
                        _ => unreachable!(),
                    };

                    if let ChainAction::Complete(written) = action {
                        ctx.complete(chain.token, written);
                    }
                }
            }
            QueueType::Event | QueueType::Rx => {}
        }
    }

    fn reset(&mut self) {
        self.pending.clear();
        self.stream = Stream::new();
    }
}

impl Snd {
    fn control(&mut self, chain: &ChainData, token: ChainToken, mem: &GuestMemory) -> ChainAction {
        let Some(hdr) = chain.read_obj::<Hdr>(0, mem) else {
            eprintln!("virtio-snd: unreadable control header");
            return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
        };

        match RequestCode::try_from(hdr.code) {
            Ok(RequestCode::PcmInfo) => {
                let Some(q) = chain.read_obj::<QueryInfo>(0, mem) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                };

                let (start, count) = (q.start_id, q.count);

                if start + count > NUM_STREAMS {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                }

                let mut resp = Hdr {
                    code: Status::Ok as u32,
                }
                .as_bytes()
                .to_vec();

                for _stream_id in start..start + count {
                    let info = PcmInfo {
                        hdr: Info { hda_fn_nid: 0 },
                        features: 0,
                        formats: 1u64 << PcmFormat::S16 as u64,
                        rates: 1u64 << PcmRate::R48000 as u64,
                        direction: Direction::Output as u8,
                        channels_min: 2,
                        channels_max: 2,
                        padding: [0; 5],
                    };
                    resp.extend_from_slice(info.as_bytes());
                }

                ChainAction::Complete(chain.write_response(&resp, mem))
            }
            Ok(RequestCode::PcmSetParams) => {
                let Some(p) = chain.read_obj::<PcmSetParams>(0, mem) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                };

                if p.hdr.stream_id != STREAM_ID || matches!(self.stream.state, StreamState::Running) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                }

                if p.format != PcmFormat::S16 as u8
                    || p.rate != PcmRate::R48000 as u8
                    || p.channels != 2
                    || p.features != 0
                {
                    return ChainAction::Complete(self.respond_status(chain, Status::NotSupp, mem));
                }

                let (period, buffer) = (p.period_bytes, p.buffer_bytes);
                if period == 0 || buffer % period != 0 || period % 4 != 0 {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                }

                self.stream.buffer_bytes = buffer;
                self.stream.period_scratch = vec![0; period as usize];
                self.stream.channels = p.channels;
                self.stream.format = p.format;
                self.stream.rate = p.rate;
                self.stream.state = StreamState::ParamsSet;

                ChainAction::Complete(self.respond_status(chain, Status::Ok, mem))
            }
            Ok(RequestCode::PcmPrepare) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, mem) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                };

                if p.stream_id != STREAM_ID || matches!(self.stream.state, StreamState::Initial | StreamState::Running)
                {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                }

                self.stream.state = StreamState::Prepared;
                ChainAction::Complete(self.respond_status(chain, Status::Ok, mem))
            }
            Ok(RequestCode::PcmStart) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, mem) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                };

                if p.stream_id != STREAM_ID || !matches!(self.stream.state, StreamState::Prepared) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                }

                self.stream.state = StreamState::Running;
                ChainAction::Complete(self.respond_status(chain, Status::Ok, mem))
            }
            Ok(RequestCode::PcmStop) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, mem) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                };

                if p.stream_id != STREAM_ID || !matches!(self.stream.state, StreamState::Running) {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                }

                self.stream.state = StreamState::Prepared;
                ChainAction::Complete(self.respond_status(chain, Status::Ok, mem))
            }
            Ok(RequestCode::PcmRelease) => {
                let Some(p) = chain.read_obj::<PcmHdr>(0, mem) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                };

                if p.stream_id != STREAM_ID
                    || !matches!(self.stream.state, StreamState::ParamsSet | StreamState::Prepared)
                {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                }

                let written = self.respond_status(chain, Status::Ok, mem);
                self.stream.state = StreamState::Initial;

                if self.pending.is_empty() {
                    ChainAction::Complete(written)
                } else {
                    self.pending_release = Some(PendingRelease { token, written });
                    ChainAction::Deferred
                }
            }
            Ok(RequestCode::ChmapInfo) => {
                let Some(q) = chain.read_obj::<QueryInfo>(0, mem) else {
                    return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                };
                let (start, count) = (q.start_id, q.count);
                match start.checked_add(count) {
                    Some(end) if end <= NUM_CHMAPS => {}
                    _ => {
                        return ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem));
                    }
                }

                let mut resp = Hdr {
                    code: Status::Ok as u32,
                }
                .as_bytes()
                .to_vec();

                for _ in start..start + count {
                    let mut positions = [ChmapPosition::None as u8; CHMAP_MAX_SIZE];
                    positions[0] = ChmapPosition::Fl as u8;
                    positions[1] = ChmapPosition::Fr as u8;
                    let info = ChmapInfo {
                        hdr: Info { hda_fn_nid: 0 },
                        direction: Direction::Output as u8,
                        channels: 2,
                        positions,
                    };
                    resp.extend_from_slice(info.as_bytes());
                }

                ChainAction::Complete(chain.write_response(&resp, mem))
            }
            Ok(RequestCode::JackInfo | RequestCode::JackRemap) => {
                ChainAction::Complete(self.respond_status(chain, Status::NotSupp, mem))
            }
            Err(v) => {
                eprintln!("virtio-snd: unknown control code: 0x{:x}", v.number);
                ChainAction::Complete(self.respond_status(chain, Status::BadMsg, mem))
            }
        }
    }

    fn submit_period(&mut self, chain: &ChainData, token: ChainToken, mem: &GuestMemory) -> ChainAction {
        const PAYLOAD_OFFSET: usize = size_of::<PcmXfer>();

        let Some(_pcm_xfer) = chain.read_obj::<PcmXfer>(0, mem) else {
            return ChainAction::Complete(0);
        };

        if chain
            .read_at(PAYLOAD_OFFSET, &mut self.stream.period_scratch, mem)
            .is_none()
        {
            return ChainAction::Complete(0);
        };

        if !self.period_sink.push(self.next_period, &self.stream.period_scratch) {
            return ChainAction::Complete(0);
        }

        let written = chain.write_response(
            PcmStatus {
                status: Status::Ok as u32,
                latency_bytes: 0, // TODO: get from CoreAudio,
            }
            .as_bytes(),
            mem,
        );

        self.pending.push_back(PendingPeriod {
            seq: self.next_period,
            token,
            written,
        });

        self.next_period = self.next_period.wrapping_add(1);

        ChainAction::Deferred
    }

    fn respond_status(&self, chain: &ChainData, status: Status, mem: &GuestMemory) -> u32 {
        let hdr = Hdr { code: status as u32 };
        chain.write_response(hdr.as_bytes(), mem)
    }
}

pub enum ExternalEvent {
    PeriodElapsed(u64),
}

impl ExternalEventHandler for Snd {
    type Event<'a> = ExternalEvent;

    fn on_event(&mut self, event: ExternalEvent, ctx: &mut DeviceContext<'_>) {
        match event {
            ExternalEvent::PeriodElapsed(seq) => {
                while let Some(front) = self.pending.front() {
                    if front.seq > seq {
                        break;
                    }
                    let p = self.pending.pop_front().unwrap();
                    ctx.complete(p.token, p.written);
                }
                if self.pending.is_empty() {
                    if let Some(r) = self.pending_release.take() {
                        ctx.complete(r.token, r.written);
                    }
                }
            }
        }
    }
}
