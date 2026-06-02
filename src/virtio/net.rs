use crate::virtio::{common, device::Device, virtq};
use applevisor::{error::Result, memory::Memory};
use num_enum::TryFromPrimitive;
use std::collections::VecDeque;

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

impl NetHeader {
    fn new(addr: u64, mem: &mut Memory) -> Result<NetHeader> {
        Ok(NetHeader {
            flags: mem.read_u8(addr)?,
            gso_type: mem.read_u8(addr + 1)?,
            hdr_len: mem.read_u16(addr + 2)?,
            gso_size: mem.read_u16(addr + 4)?,
            csum_start: mem.read_u16(addr + 6)?,
            csum_offset: mem.read_u16(addr + 8)?,
            num_buffers: mem.read_u16(addr + 10)?,
        })
    }
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

    fn config(&self, offset: u64) -> u32 {
        let v = match offset {
            0..6 => self.mac[offset as usize] as u32,
            _ => 0,
        };
        eprintln!("net config read: offset={}, value=0x{:08x}", offset, v);
        v
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

        let mut packet = Vec::with_capacity(size_of::<NetHeader>() + frame.len());
        packet.extend_from_slice(&self.plain_rx_hdr());
        packet.extend_from_slice(frame);

        let written = self.write_chain(queue, head_idx, &packet, mem);

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
        let mut frame = Vec::new();
        let mut cur = Some(head_idx);
        while let Some(idx) = cur {
            let desc = queue.read_desc(idx, mem);
            let mut buf = vec![0u8; desc.len as usize];
            mem.read(desc.addr, &mut buf).unwrap();
            frame.extend_from_slice(&buf);
            cur = desc.next();
        }

        if frame.len() < size_of::<NetHeader>() {
            eprintln!("TX: chain too short, {} bytes", frame.len());
            return Some(0);
        }

        let eth = &frame[size_of::<NetHeader>()..];

        self.tx_frames.push_front(eth.to_vec());

        Some(0)
    }

    fn plain_rx_hdr(&self) -> [u8; size_of::<NetHeader>()] {
        let mut out = [0u8; size_of::<NetHeader>()];

        out[10..12].copy_from_slice(&1u16.to_le_bytes());

        out
    }

    fn write_chain(
        &self,
        queue: &virtq::Queue,
        head_idx: u16,
        data: &[u8],
        mem: &mut Memory,
    ) -> usize {
        let mut written = 0usize;
        let mut cur = Some(head_idx);

        while written < data.len() {
            let idx = cur.unwrap();
            let desc = queue.read_desc(idx, mem);

            let n = (data.len() - written).min(desc.len as usize);

            mem.write(desc.addr, &data[written..written + n]).unwrap();

            written += n;
            cur = desc.next();
        }

        written
    }
}
