use crate::virtio::{common, device::Device};
use applevisor::{error::Result, memory::Memory};
use num_enum::TryFromPrimitive;

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

pub struct Net {}

impl Net {
    pub fn new() -> Net {
        Net {}
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
            0..MAC_LEN => MAC[offset as usize] as u32,
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
        queue: &super::virtq::Queue,
        head_idx: u16,
        mem: &mut Memory,
    ) -> u32 {
        let head_desc = queue.read_desc(head_idx, mem);
        let net_header = NetHeader::new(head_desc.addr, mem).unwrap();
        let queue_type = QueueType::try_from(q_idx).unwrap();

        let mut cur = head_desc.next();
        let mut written_len: u32 = 0;

        while let Some(cur_desc_idx) = cur {
            let cur_desc = queue.read_desc(cur_desc_idx, mem);

            let next = cur_desc.next();

            match queue_type {
                QueueType::Rx => {}
                QueueType::Tx => {
                    let mut buf = vec![0u8; cur_desc.len as usize];
                    mem.read(cur_desc.addr, &mut buf).unwrap();
                    eprintln!("TX: buf=[{:?}]", buf);
                }
            }

            cur = next;
        }

        written_len
    }
}
