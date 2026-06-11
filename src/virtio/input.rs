use crate::virtio::{MmioTransport, common, device::Device, input::keys::*, virtq};
use applevisor::memory::Memory;
use num_enum::TryFromPrimitive;
use zerocopy::{FromBytes, Immutable, IntoBytes};

pub mod keys;

const DEVICE_ID: u32 = 18;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(usize)]
enum QueueType {
    Event = 0,
    Status = 1,
}

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-2450006
#[derive(IntoBytes, FromBytes, Immutable)]
#[repr(C, packed)]
struct Event {
    r#type: u16,
    code: u16,
    value: u32,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C)]
struct InputConfig {
    select: u8,
    subsel: u8,
    size: u8,
    reserved: [u8; 5],
    u: [u8; 128],
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct InputDevids {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[derive(IntoBytes, FromBytes, Immutable, Debug, Copy, Clone)]
#[repr(C, packed)]
struct AbsInfo {
    min: u32,
    max: u32,
    fuzz: u32,
    flat: u32,
    res: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
enum InputConfigSelect {
    IdName = 0x01,
    IdSerial = 0x02,
    IdDevids = 0x03,
    PropBits = 0x10,
    EvBits = 0x11,
    AbsInfo = 0x12,
}

pub enum InputKind {
    Keyboard,
    Tablet { width: u32, height: u32 },
}

pub struct Input {
    kind: InputKind,
    select: u8,
    subsel: u8,
    free_rx_buffers: Vec<u16>,
}

fn set_bit(bitmap: &mut [u8], bit: usize) {
    bitmap[bit / 8] |= 1 << (bit % 8);
}

impl Input {
    pub fn keyboard() -> Input {
        Input {
            kind: InputKind::Keyboard,
            select: 0,
            subsel: 0,
            free_rx_buffers: Vec::new(),
        }
    }

    pub fn tablet(width: u32, height: u32) -> Input {
        Input {
            kind: InputKind::Tablet { width, height },
            select: 0,
            subsel: 0,
            free_rx_buffers: Vec::new(),
        }
    }

    fn config(&self) -> InputConfig {
        let mut cfg = InputConfig {
            select: self.select,
            subsel: self.subsel,
            size: 0,
            reserved: [0; 5],
            u: [0; 128],
        };

        match InputConfigSelect::try_from(self.select) {
            Ok(InputConfigSelect::IdName) => {
                let name = match self.kind {
                    InputKind::Keyboard => "varmint keyboard",
                    InputKind::Tablet { .. } => "varmint tablet",
                };
                cfg.size = name.len() as u8;
                cfg.u[..name.len()].copy_from_slice(name.as_bytes());
            }

            Ok(InputConfigSelect::IdDevids) => {
                let devids = InputDevids {
                    bustype: BUS_VIRTUAL,
                    vendor: 0x1234,
                    product: 1,
                    version: 1,
                };

                let bytes = devids.as_bytes();
                cfg.size = bytes.len() as u8;
                cfg.u[..bytes.len()].copy_from_slice(bytes);
            }

            Ok(InputConfigSelect::AbsInfo) => {
                if let InputKind::Tablet { width, height } = self.kind {
                    let abs = match self.subsel as u16 {
                        ABS_X => Some(AbsInfo {
                            min: 0,
                            max: width - 1,
                            fuzz: 0,
                            flat: 0,
                            res: 0,
                        }),
                        ABS_Y => Some(AbsInfo {
                            min: 0,
                            max: height - 1,
                            fuzz: 0,
                            flat: 0,
                            res: 0,
                        }),
                        _ => None,
                    };

                    if let Some(abs) = abs {
                        let bytes = abs.as_bytes();
                        cfg.size = bytes.len() as u8;
                        cfg.u[..bytes.len()].copy_from_slice(bytes);
                    }
                }
            }

            Ok(InputConfigSelect::EvBits) => match self.kind {
                InputKind::Keyboard => match self.subsel as u16 {
                    EV_SYN => {
                        set_bit(&mut cfg.u, SYN_REPORT as usize);
                        cfg.size = 1;
                    }
                    EV_KEY => {
                        cfg.size = fill_key_bits(&mut cfg.u);
                    }
                    _ => {}
                },

                InputKind::Tablet { .. } => match self.subsel as u16 {
                    EV_SYN => {
                        set_bit(&mut cfg.u, SYN_REPORT as usize);
                        cfg.size = 1;
                    }
                    EV_KEY => {
                        set_bit(&mut cfg.u, BTN_LEFT as usize);
                        set_bit(&mut cfg.u, BTN_RIGHT as usize);
                        set_bit(&mut cfg.u, BTN_MIDDLE as usize);

                        cfg.size = (BTN_MIDDLE / 8 + 1) as u8;
                    }
                    EV_ABS => {
                        set_bit(&mut cfg.u, ABS_X as usize);
                        set_bit(&mut cfg.u, ABS_Y as usize);

                        cfg.size = 1;
                    }
                    _ => {}
                },
            },

            Ok(InputConfigSelect::PropBits) => {
                if matches!(self.kind, InputKind::Tablet { .. }) {
                    set_bit(&mut cfg.u, INPUT_PROP_POINTER as usize);
                    cfg.size = 1;
                }
            }

            Ok(InputConfigSelect::IdSerial) => {
                cfg.size = 0;
            }

            Err(e) => {
                eprintln!("unexpected select: {}", e);
                cfg.size = 0;
            }
        }

        cfg
    }
}

impl Device for Input {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let offset = offset as usize;
        data.copy_from_slice(&self.config().as_bytes()[offset..offset + data.len()]);
    }

    fn write_config(&mut self, offset: u64, data: &[u8]) {
        for (i, &byte) in data.iter().enumerate() {
            match offset + i as u64 {
                0 => self.select = byte,
                1 => self.subsel = byte,
                _ => {}
            }
        }
    }

    fn num_queues(&self) -> u16 {
        2
    }

    fn process_chain(
        &mut self,
        q_idx: usize,
        _queue: &virtq::Queue,
        head_idx: u16,
        _mem: &mut Memory,
    ) -> Option<u32> {
        let queue_type = QueueType::try_from(q_idx).unwrap();
        match queue_type {
            QueueType::Event => {
                self.free_rx_buffers.push(head_idx);
                None
            }
            QueueType::Status => Some(0),
        }
    }

    fn handle_external(
        &mut self,
        queues: &[virtq::Queue],
        data: &[u8],
        mem: &mut Memory,
    ) -> Option<virtq::Completion> {
        let head_idx = self.free_rx_buffers.pop()?;
        let queue = &queues[QueueType::Event as usize];

        let chain = queue.collect_chain(head_idx, mem).unwrap();

        let written = chain.write_response(data, mem);

        Some(virtq::Completion {
            queue_idx: QueueType::Event as u16,
            head_idx,
            used_len: written as u32,
        })
    }

    fn reset(&mut self) {
        self.select = 0;
        self.subsel = 0;
        self.free_rx_buffers.clear();
    }
}

impl MmioTransport<Input> {
    fn push_event(&mut self, r#type: u16, code: u16, value: u32, mem: &mut Memory) -> bool {
        let event = Event {
            r#type,
            code,
            value,
        };

        let ok = self.deliver_external(event.as_bytes(), mem);

        if !ok {
            eprintln!(
                "virtio-input: dropped event type={} code={} value={}",
                r#type, code, value
            );
        }

        ok
    }

    pub fn push_key(&mut self, code: u16, pressed: bool, mem: &mut Memory) {
        let value = if pressed { 1 } else { 0 };

        self.push_event(EV_KEY, code, value, mem);
        self.push_event(EV_SYN, SYN_REPORT, 0, mem);
    }

    pub fn push_abs_position(&mut self, x: u32, y: u32, mem: &mut Memory) {
        self.push_event(EV_ABS, ABS_X, x, mem);
        self.push_event(EV_ABS, ABS_Y, y, mem);
        self.push_event(EV_SYN, SYN_REPORT, 0, mem);
    }

    pub fn push_pointer_button(&mut self, button: u16, pressed: bool, mem: &mut Memory) {
        let value = if pressed { 1 } else { 0 };

        self.push_event(EV_KEY, button, value, mem);
        self.push_event(EV_SYN, SYN_REPORT, 0, mem);
    }
}
