use crate::{
    memory::GuestMemory,
    virtio::chain::{ChainData, Seg},
};
use applevisor::error::Result;
use std::sync::atomic::{Ordering, fence};

pub mod flags {
    pub const DESC_F_NEXT: u16 = 1;
    pub const DESC_F_WRITE: u16 = 2;
    pub const DESC_F_INDIRECT: u16 = 4;
}

#[derive(Debug)]
#[repr(C)]
pub struct Desc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl Desc {
    pub fn new(offset: u64, mem: &GuestMemory) -> Result<Desc> {
        Ok(Desc {
            addr: mem.read_u64(offset)?,
            len: mem.read_u32(offset + 8)?,
            flags: mem.read_u16(offset + 8 + 4)?,
            next: mem.read_u16(offset + 8 + 4 + 2)?,
        })
    }

    pub fn next(&self) -> Option<u16> {
        (self.flags & flags::DESC_F_NEXT != 0).then_some(self.next)
    }

    pub fn is_writable(&self) -> bool {
        (self.flags & flags::DESC_F_WRITE) != 0
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct AvailHeader {
    pub flags: u16,
    pub idx: u16,
}

#[derive(Debug)]
#[repr(C)]
pub struct UsedHeader {
    pub flags: u16,
    pub idx: u16,
}

#[derive(Default, Clone)]
pub struct Queue {
    pub ready: bool,

    size: u16,

    desc_addr: u64,
    avail_addr: u64,
    used_addr: u64,

    last_avail_idx: u16,
    last_used_idx: u16,
}

impl Queue {
    pub fn new(ready: bool, size: u16, desc_addr: u64, avail_addr: u64, used_addr: u64) -> Queue {
        Queue {
            ready,
            size,
            desc_addr,
            avail_addr,
            used_addr,
            last_avail_idx: 0,
            last_used_idx: 0,
        }
    }

    pub fn pop_chain(&mut self, mem: &GuestMemory) -> Option<u16> {
        const AVAIL_HEADER_SIZE: u64 = size_of::<AvailHeader>() as u64;

        if !self.ready || self.size == 0 {
            return None;
        }

        let avail_idx = mem.read_u16(self.avail_addr + 2).ok()?;
        if avail_idx == self.last_avail_idx {
            return None;
        }

        // TBR: might be unnecessary, MMIO trap likely already does some heavy sync
        fence(Ordering::Acquire);

        let ring_idx = self.last_avail_idx % self.size;
        let head_idx = mem
            .read_u16(self.avail_addr + AVAIL_HEADER_SIZE + (ring_idx * 2) as u64)
            .ok()?;

        if head_idx >= self.size {
            eprintln!(
                "virtq: bad avail head={} size={} last_avail_idx={} avail_idx={}",
                head_idx, self.size, self.last_avail_idx, avail_idx
            );
            return None;
        }

        self.last_avail_idx = self.last_avail_idx.wrapping_add(1);
        Some(head_idx)
    }

    pub fn read_desc(&self, idx: u16, mem: &GuestMemory) -> Option<Desc> {
        if idx >= self.size {
            eprintln!("virtq: bad descriptor index: {}", idx);
            return None;
        }
        Desc::new(self.desc_addr + idx as u64 * size_of::<Desc>() as u64, mem).ok()
    }

    pub fn push_used(&mut self, mem: &GuestMemory, head_idx: u16, written_len: u32) {
        const USED_HEADER_SIZE: u64 = size_of::<UsedHeader>() as u64;

        if !self.ready || self.size == 0 {
            return;
        }

        if head_idx >= self.size {
            eprintln!("virtq: refusing to push bad used head={} size={}", head_idx, self.size);
            return;
        }

        let ring_idx = self.last_used_idx % self.size;
        let ring_addr = self.used_addr + USED_HEADER_SIZE + (ring_idx * 8) as u64;
        mem.write_u32(ring_addr, head_idx as u32).unwrap();
        mem.write_u32(ring_addr + 4, written_len).unwrap();

        fence(Ordering::Release);

        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        mem.write_u16(self.used_addr + 2, self.last_used_idx).unwrap();
    }

    pub fn collect_chain(&self, head_idx: u16, mem: &GuestMemory) -> Option<ChainData> {
        let mut out = ChainData::default();

        let mut cur = Some(head_idx);
        let mut seen_writable = false;
        let mut count = 0usize;

        while let Some(idx) = cur {
            if count >= self.size as usize {
                eprintln!("virtq: descriptor chain is too long or loops");
                return None;
            }
            count += 1;

            let desc = self.read_desc(idx, mem)?;

            if desc.flags & flags::DESC_F_INDIRECT != 0 {
                eprintln!("virtq: indirect descriptors are not supported");
                return None;
            }

            let seg = Seg {
                addr: desc.addr,
                len: desc.len,
            };

            if desc.is_writable() {
                seen_writable = true;
                out.writable.push(seg);
            } else {
                if seen_writable {
                    eprintln!("virtq: readable descriptor after writable descriptor");
                    return None;
                }

                out.readable.push(seg);
                out.readable_len += desc.len as usize;
            }

            cur = desc.next();
        }

        Some(out)
    }

    pub fn reset(&mut self) {
        self.ready = false;
        self.size = 0;
        self.desc_addr = 0;
        self.avail_addr = 0;
        self.used_addr = 0;
        self.last_avail_idx = 0;
        self.last_used_idx = 0;
    }
}
