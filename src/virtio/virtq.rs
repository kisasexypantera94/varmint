use applevisor::{error::Result, memory::Memory};

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
    pub fn new(offset: u64, mem: &Memory) -> Result<Desc> {
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

impl AvailHeader {
    pub fn new(offset: u64, mem: &Memory) -> Result<AvailHeader> {
        Ok(AvailHeader {
            flags: mem.read_u16(offset)?,
            idx: mem.read_u16(offset + 2)?,
        })
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct UsedHeader {
    pub flags: u16,
    pub idx: u16,
}

impl UsedHeader {
    pub fn new(offset: u64, mem: &Memory) -> Result<UsedHeader> {
        Ok(UsedHeader {
            flags: mem.read_u16(offset)?,
            idx: mem.read_u16(offset + 2)?,
        })
    }
}

pub struct Completion {
    pub queue_idx: u16,
    pub head_idx: u16,
    pub used_len: u32,
}

#[derive(Debug, Copy, Clone)]
pub struct WritableDesc {
    pub addr: u64,
    pub len: u32,
}

#[derive(Debug)]
pub struct ChainData {
    pub readable: Vec<u8>,
    pub writable: Vec<WritableDesc>,
}

impl ChainData {
    pub fn write_response(&self, bytes: &[u8], mem: &mut Memory) -> u32 {
        let mut written = 0usize;

        for desc in &self.writable {
            if written == bytes.len() {
                break;
            }

            let n = (desc.len as usize).min(bytes.len() - written);
            mem.write(desc.addr, &bytes[written..written + n]).unwrap();
            written += n;
        }

        written as u32
    }
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

    pub fn pop_chain(&mut self, mem: &Memory) -> Option<u16> {
        const AVAIL_HEADER_SIZE: u64 = size_of::<AvailHeader>() as u64;

        let avail_idx = mem.read_u16(self.avail_addr + 2).ok()?;
        if avail_idx == self.last_avail_idx {
            return None;
        }

        let ring_idx = self.last_avail_idx % self.size;
        let head_idx = mem
            .read_u16(self.avail_addr + AVAIL_HEADER_SIZE + (ring_idx * 2) as u64)
            .ok()?;

        self.last_avail_idx = self.last_avail_idx.wrapping_add(1);
        Some(head_idx)
    }

    pub fn read_desc(&self, idx: u16, mem: &Memory) -> Desc {
        assert!(idx < self.size, "bad descriptor index: {}", idx);
        Desc::new(self.desc_addr + idx as u64 * size_of::<Desc>() as u64, mem).unwrap()
    }

    pub fn push_used(&mut self, mem: &mut Memory, head_idx: u16, written_len: u32) {
        const USED_HEADER_SIZE: u64 = size_of::<UsedHeader>() as u64;

        let ring_idx = self.last_used_idx % self.size;
        let ring_addr = self.used_addr + USED_HEADER_SIZE + (ring_idx * 8) as u64;
        mem.write_u32(ring_addr, head_idx as u32).unwrap();
        mem.write_u32(ring_addr + 4, written_len).unwrap();

        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        mem.write_u16(self.used_addr + 2, self.last_used_idx)
            .unwrap();
    }

    pub fn collect_chain(&self, head_idx: u16, mem: &Memory) -> Option<ChainData> {
        let mut out = ChainData {
            readable: Vec::new(),
            writable: Vec::new(),
        };

        let mut cur = Some(head_idx);
        let mut seen_writable = false;
        let mut count = 0usize;

        while let Some(idx) = cur {
            if count >= self.size as usize {
                eprintln!("virtq: descriptor chain is too long or loops");
                return None;
            }
            count += 1;

            let desc = self.read_desc(idx, mem);

            if desc.flags & flags::DESC_F_INDIRECT != 0 {
                eprintln!("virtq: indirect descriptors are not supported");
                return None;
            }

            if desc.flags & flags::DESC_F_WRITE != 0 {
                seen_writable = true;
                out.writable.push(WritableDesc {
                    addr: desc.addr,
                    len: desc.len,
                });
            } else {
                if seen_writable {
                    eprintln!("virtq: readable descriptor after writable descriptor");
                    return None;
                }

                let old_len = out.readable.len();
                out.readable.resize(old_len + desc.len as usize, 0);
                mem.read(desc.addr, &mut out.readable[old_len..]).ok()?;
            }

            cur = desc.next();
        }

        Some(out)
    }
}
