use crate::{
    memory::GuestMemory,
    virtio::{
        chain::ChainData,
        common,
        device::{ChainAction, ChainToken, Device, Effect, ExternalEventHandler},
    },
};
use num_enum::TryFromPrimitive;
use std::collections::VecDeque;
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes};

mod feature {
    pub const MAC: u64 = 1 << 5;
}

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-2350001
const DEVICE_ID: u32 = 1;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(usize)]
enum QueueType {
    Rx = 0,
    Tx = 1,
}

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-2450006
#[derive(IntoBytes, FromBytes, Immutable)]
#[repr(C, packed)]
struct NetHeader {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    gso_size: u16,
    csum_start: u16,
    csum_offset: u16,
    num_buffers: u16,
}

pub struct Net {
    mac: [u8; 6],
    tx_frames: VecDeque<Vec<u8>>,
}

impl Net {
    pub fn new(mac: [u8; 6]) -> Net {
        Net {
            mac,
            tx_frames: VecDeque::new(),
        }
    }
}

impl Device for Net {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1 | feature::MAC
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let offset = offset as usize;
        data.copy_from_slice(&self.mac[offset..offset + data.len()]);
    }

    fn num_queues(&self) -> u16 {
        2
    }

    fn delivery_queues(&self) -> &[u16] {
        &[QueueType::Rx as u16]
    }

    fn process_chain(
        &mut self,
        queue_idx: usize,
        chain: &ChainData,
        _token: ChainToken,
        mem: &GuestMemory,
    ) -> ChainAction {
        match QueueType::try_from(queue_idx).unwrap() {
            QueueType::Rx => ChainAction::Complete(0),
            QueueType::Tx => ChainAction::Complete(self.handle_tx(chain, mem)),
        }
    }

    fn pop_external(&mut self) -> Option<Vec<u8>> {
        self.tx_frames.pop_back()
    }

    fn reset(&mut self) {
        self.tx_frames.clear();
    }
}

impl Net {
    fn handle_tx(&mut self, chain: &ChainData, mem: &GuestMemory) -> u32 {
        const HEADER_LEN: usize = size_of::<NetHeader>();

        let total = chain.readable_len();
        if total < HEADER_LEN {
            eprintln!("TX: chain too short, {} bytes", total);
            return 0;
        }

        let mut eth = vec![0u8; total - HEADER_LEN];
        if chain.read_at(HEADER_LEN, &mut eth, mem).is_none() {
            eprintln!("TX: failed to read frame from guest memory");
            return 0;
        }

        self.tx_frames.push_front(eth);

        0
    }

    fn plain_rx_hdr() -> NetHeader {
        let mut hdr = NetHeader::new_zeroed();
        hdr.num_buffers = 1;
        hdr
    }
}

impl ExternalEventHandler for Net {
    type Event<'a> = &'a [u8];

    fn on_event(&mut self, frame: Self::Event<'_>, mut emit: impl FnMut(Effect)) {
        emit(Effect::Deliver {
            queue_idx: QueueType::Rx as usize,
            parts: &[Net::plain_rx_hdr().as_bytes(), frame],
        })
    }
}
