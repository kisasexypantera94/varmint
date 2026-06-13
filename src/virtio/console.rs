use crate::virtio::{
    chain::ChainData,
    common,
    device::{ChainAction, ChainToken, Device, Effect, ExternalEventHandler},
};
use applevisor::memory::Memory;
use std::collections::VecDeque;
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes};

const DEVICE_ID: u32 = 3;

mod feature {
    pub const MULTIPORT: u64 = 1 << 1;
}

const Q_PORT0_RX: usize = 0;
const Q_PORT0_TX: usize = 1;
const Q_CONTROL_RX: usize = 2;
const Q_CONTROL_TX: usize = 3;
const Q_CLIP_RX: usize = 4;
const Q_CLIP_TX: usize = 5;

const NUM_QUEUES: u16 = 6;

const CLIP_PORT: u32 = 1;
const PORT_NAME: &[u8] = b"clipboard";

#[derive(IntoBytes, FromBytes, Immutable)]
#[repr(C, packed)]
struct ConsoleConfig {
    cols: u16,
    rows: u16,
    max_nr_ports: u32,
    emerg_wr: u32,
}

impl ConsoleConfig {
    fn new() -> ConsoleConfig {
        let mut c = ConsoleConfig::new_zeroed();
        c.max_nr_ports = 2;
        c
    }
}

#[derive(IntoBytes, FromBytes, Immutable, Clone, Copy)]
#[repr(C, packed)]
struct Control {
    id: u32,
    event: u16,
    value: u16,
}

mod ctrl_event {
    pub const DEVICE_READY: u16 = 0;
    pub const DEVICE_ADD: u16 = 1;
    pub const PORT_READY: u16 = 3;
    pub const CONSOLE_PORT: u16 = 4;
    pub const PORT_OPEN: u16 = 6;
    pub const PORT_NAME: u16 = 7;
}

const FRAME_LEN_PREFIX: usize = 4;
const RX_CHUNK: usize = 4096;

pub struct Console {
    config: ConsoleConfig,
    tx_to_host: VecDeque<Vec<u8>>,
    rx_to_guest: VecDeque<Vec<u8>>,
    ctrl_to_guest: VecDeque<Vec<u8>>,
    port_open: bool,
}

impl Console {
    pub fn new() -> Console {
        Console {
            config: ConsoleConfig::new(),
            tx_to_host: VecDeque::new(),
            rx_to_guest: VecDeque::new(),
            ctrl_to_guest: VecDeque::new(),
            port_open: false,
        }
    }

    fn queue_control(&mut self, id: u32, event: u16, value: u16) {
        let c = Control { id, event, value };
        self.ctrl_to_guest.push_back(c.as_bytes().to_vec());
    }

    fn queue_control_with_name(&mut self, id: u32, event: u16, value: u16, name: &[u8]) {
        let c = Control { id, event, value };
        let mut buf = c.as_bytes().to_vec();
        buf.extend_from_slice(name);
        self.ctrl_to_guest.push_back(buf);
    }

    fn handle_control_tx(&mut self, chain: &ChainData, mem: &Memory) {
        let Some(c) = chain.read_obj::<Control>(0, mem) else {
            return;
        };
        let id = c.id;
        let event = c.event;
        let value = c.value;

        match event {
            ctrl_event::DEVICE_READY => {
                self.queue_control(CLIP_PORT, ctrl_event::DEVICE_ADD, 0);
            }
            ctrl_event::PORT_READY => {
                if id == CLIP_PORT {
                    self.queue_control(CLIP_PORT, ctrl_event::CONSOLE_PORT, 0);
                    self.queue_control_with_name(CLIP_PORT, ctrl_event::PORT_NAME, 1, PORT_NAME);
                    self.queue_control(CLIP_PORT, ctrl_event::PORT_OPEN, 1);
                }
            }
            ctrl_event::PORT_OPEN => {
                if id == CLIP_PORT {
                    self.port_open = value != 0;
                }
            }
            _ => {}
        }
    }

    fn handle_clip_tx(&mut self, chain: &ChainData, mem: &Memory) -> u32 {
        let total = chain.readable_len();
        if total == 0 {
            return 0;
        }
        let mut buf = vec![0u8; total];
        if chain.read_at(0, &mut buf, mem).is_none() {
            eprintln!("console clip TX: failed to read {total} bytes from guest memory");
            return 0;
        }
        self.tx_to_host.push_front(buf);
        0
    }

    fn enqueue_rx(&mut self, payload: &[u8]) {
        let mut framed = Vec::with_capacity(FRAME_LEN_PREFIX + payload.len());
        framed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        framed.extend_from_slice(payload);
        for chunk in framed.chunks(RX_CHUNK) {
            self.rx_to_guest.push_back(chunk.to_vec());
        }
    }
}

impl Device for Console {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1 | feature::MULTIPORT
    }

    fn num_queues(&self) -> u16 {
        NUM_QUEUES
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let offset = offset as usize;
        let bytes = self.config.as_bytes();
        let end = (offset + data.len()).min(bytes.len());
        if offset < end {
            let n = end - offset;
            data[..n].copy_from_slice(&bytes[offset..end]);
        }
    }

    fn delivery_queues(&self) -> &[u16] {
        &[Q_PORT0_RX as u16, Q_CONTROL_RX as u16, Q_CLIP_RX as u16]
    }

    fn process_chain(
        &mut self,
        queue_idx: usize,
        chain: &ChainData,
        _token: ChainToken,
        mem: &mut Memory,
    ) -> ChainAction {
        match queue_idx {
            Q_CONTROL_TX => {
                self.handle_control_tx(chain, mem);
                ChainAction::Complete(0)
            }
            Q_CLIP_TX => ChainAction::Complete(self.handle_clip_tx(chain, mem)),
            Q_PORT0_TX => ChainAction::Complete(0),
            _ => ChainAction::Complete(0),
        }
    }

    fn pop_external(&mut self) -> Option<Vec<u8>> {
        self.tx_to_host.pop_back()
    }

    fn reset(&mut self) {
        self.tx_to_host.clear();
        self.rx_to_guest.clear();
        self.ctrl_to_guest.clear();
        self.port_open = false;
    }
}

pub enum ExternalEvent<'a> {
    HostClipboard(&'a [u8]),
    RxAvailable,
}

impl ExternalEventHandler for Console {
    type Event<'a> = ExternalEvent<'a>;

    fn on_event(&mut self, event: ExternalEvent<'_>, mut emit: impl FnMut(Effect)) {
        if let ExternalEvent::HostClipboard(payload) = event {
            if self.port_open {
                self.enqueue_rx(payload);
            }
        }

        while let Some(msg) = self.ctrl_to_guest.front() {
            emit(Effect::Deliver {
                queue_idx: Q_CONTROL_RX,
                parts: &[msg],
            });
            self.ctrl_to_guest.pop_front();
        }

        while let Some(chunk) = self.rx_to_guest.front() {
            emit(Effect::Deliver {
                queue_idx: Q_CLIP_RX,
                parts: &[chunk],
            });
            self.rx_to_guest.pop_front();
        }
    }
}
