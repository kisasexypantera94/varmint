use crate::virtio::{
    common,
    device::{Device, ExternalInputHandler},
    virtq,
};
use applevisor::memory::Memory;
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

    fn async_queues(&self) -> &[u16] {
        &[QueueType::Rx as u16]
    }

    fn process_chain(
        &mut self,
        q_idx: usize,
        queue: &virtq::Queue,
        head_idx: u16,
        mem: &mut Memory,
    ) -> Option<u32> {
        match QueueType::try_from(q_idx).unwrap() {
            QueueType::Rx => None,
            QueueType::Tx => self.handle_tx(queue, head_idx, mem),
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
    fn handle_tx(&mut self, queue: &virtq::Queue, head_idx: u16, mem: &mut Memory) -> Option<u32> {
        let virtq::ChainData { readable, .. } = queue.collect_chain(head_idx, mem)?;

        if readable.len() < size_of::<NetHeader>() {
            eprintln!("TX: chain too short, {} bytes", readable.len());
            return Some(0);
        }

        let (_, eth) = NetHeader::read_from_prefix(&readable).ok()?;

        self.tx_frames.push_front(eth.to_vec());

        Some(0)
    }

    fn plain_rx_hdr() -> NetHeader {
        let mut hdr = NetHeader::new_zeroed();
        hdr.num_buffers = 1;
        hdr
    }
}

impl ExternalInputHandler for Net {
    type Input<'a> = &'a [u8];

    fn encode(&mut self, frame: Self::Input<'_>, mut emit: impl FnMut(usize, &[&[u8]])) {
        emit(
            QueueType::Rx as usize,
            &[Net::plain_rx_hdr().as_bytes(), frame],
        )
    }
}
