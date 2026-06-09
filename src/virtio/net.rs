use crate::virtio::{common, device::Device, virtq};
use applevisor::memory::Memory;
use num_enum::TryFromPrimitive;
use std::collections::VecDeque;
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes};

mod feature {
    pub const MAC: u64 = 1 << 5;
}

/// 02: local-administered unicast MAC.  \
/// 56 41 52 4d = ASCII-ish "VARM".  \
/// 01 = first varmint NIC.  \
const MAC: [u8; 6] = [0x02, 0x56, 0x41, 0x52, 0x4d, 0x01];
const MAC_LEN: u64 = MAC.len() as u64;

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
    free_rx_buffers: Vec<u16>,
    tx_frames: VecDeque<Vec<u8>>,
}

impl Net {
    pub fn new(mac: [u8; 6]) -> Net {
        Net {
            mac,
            free_rx_buffers: Vec::new(),
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

    fn process_chain(
        &mut self,
        q_idx: usize,
        queue: &virtq::Queue,
        head_idx: u16,
        mem: &mut Memory,
    ) -> Option<u32> {
        let queue_type = QueueType::try_from(q_idx).unwrap();
        match queue_type {
            QueueType::Rx => self.handle_rx(head_idx),
            QueueType::Tx => self.handle_tx(queue, head_idx, mem),
        }
    }

    fn handle_external(
        &mut self,
        queues: &[virtq::Queue],
        frame: &[u8],
        mem: &mut Memory,
    ) -> Option<virtq::Completion> {
        let head_idx = self.free_rx_buffers.pop()?;
        let queue = &queues[QueueType::Rx as usize];
        let chain = queue.collect_chain(head_idx, mem).unwrap();

        let mut packet = Vec::with_capacity(size_of::<NetHeader>() + frame.len());
        packet.extend_from_slice(Net::plain_rx_hdr().as_bytes());
        packet.extend_from_slice(frame);

        let written = chain.write_response(&packet, mem);

        Some(virtq::Completion {
            queue_idx: QueueType::Rx as u16,
            head_idx,
            used_len: written as u32,
        })
    }

    fn pop_external(&mut self) -> Option<Vec<u8>> {
        self.tx_frames.pop_back()
    }
}

impl Net {
    fn handle_rx(&mut self, head_idx: u16) -> Option<u32> {
        self.free_rx_buffers.push(head_idx);
        None
    }

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
