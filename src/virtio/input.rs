use crate::virtio::{MmioTransport, common, device::Device, input::keys::*, virtq};
use applevisor::memory::Memory;
use num_enum::TryFromPrimitive;
use zerocopy::{FromBytes, Immutable, IntoBytes};

pub mod keys;

mod feature {
    pub const MAC: u64 = 1 << 5;
}

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

pub struct Input {
    select: u8,
    subsel: u8,
    free_rx_buffers: Vec<u16>,
}

fn set_bit(bitmap: &mut [u8], bit: usize) {
    bitmap[bit / 8] |= 1 << (bit % 8);
}

impl Input {
    pub fn new() -> Input {
        Input {
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
                let name = b"varmint keyboard";
                cfg.size = name.len() as u8;
                cfg.u[..name.len()].copy_from_slice(name);
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

            Ok(InputConfigSelect::EvBits) => match self.subsel as u16 {
                EV_SYN => {
                    set_bit(&mut cfg.u, SYN_REPORT as usize);
                    cfg.size = 1;
                }
                EV_KEY => {
                    cfg.size = fill_key_bits(&mut cfg.u);
                }

                _ => {}
            },

            Ok(InputConfigSelect::PropBits) => {
                cfg.size = 0;
            }

            Ok(InputConfigSelect::IdSerial) => {
                cfg.size = 0;
            }

            Ok(InputConfigSelect::AbsInfo) => {
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
        common::feature::VERSION_1 | feature::MAC
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
        queue: &virtq::Queue,
        head_idx: u16,
        mem: &mut Memory,
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
}

impl MmioTransport<Input> {
    fn push_event(&mut self, r#type: u16, code: u16, value: u32, mem: &mut Memory) {
        let event = Event {
            r#type,
            code,
            value,
        };

        self.deliver_external(event.as_bytes(), mem);
    }

    pub fn push_key(&mut self, code: u16, pressed: bool, mem: &mut Memory) {
        let value = if pressed { 1 } else { 0 };

        self.push_event(EV_KEY, code, value, mem);
        self.push_event(EV_SYN, SYN_REPORT, 0, mem);
    }
}
