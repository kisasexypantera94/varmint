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

pub fn queue_addr(lo: u32, hi: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}
