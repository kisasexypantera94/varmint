use crate::memory::GuestMemory;
use zerocopy::{FromBytes, Immutable, IntoBytes};

#[derive(Debug, Copy, Clone)]
pub struct Seg {
    pub addr: u64,
    pub len: u32,
}

#[derive(Debug, Default)]
pub struct ChainData {
    pub readable: Vec<Seg>,
    pub readable_len: usize,
    pub writable: Vec<Seg>,
}

impl ChainData {
    pub fn readable_len(&self) -> usize {
        self.readable_len
    }

    pub fn read_at(&self, mut offset: usize, buf: &mut [u8], mem: &GuestMemory) -> Option<()> {
        let end = offset.checked_add(buf.len())?;
        if end > self.readable_len {
            return None;
        }

        let mut filled = 0usize;
        for seg in &self.readable {
            if filled == buf.len() {
                break;
            }

            let seg_len = seg.len as usize;
            if offset >= seg_len {
                offset -= seg_len;
                continue;
            }

            let n = (seg_len - offset).min(buf.len() - filled);
            mem.read(seg.addr + offset as u64, &mut buf[filled..filled + n]).ok()?;
            filled += n;
            offset = 0;
        }

        (filled == buf.len()).then_some(())
    }

    pub fn read_obj<T: FromBytes + IntoBytes + Immutable>(&self, offset: usize, mem: &GuestMemory) -> Option<T> {
        let mut v = T::new_zeroed();
        self.read_at(offset, v.as_mut_bytes(), mem)?;
        Some(v)
    }

    pub fn write_response(&self, bytes: &[u8], mem: &GuestMemory) -> u32 {
        let mut written = 0usize;

        for seg in &self.writable {
            if written == bytes.len() {
                break;
            }

            let n = (seg.len as usize).min(bytes.len() - written);
            mem.write(seg.addr, &bytes[written..written + n]).unwrap();
            written += n;
        }

        written as u32
    }

    pub fn write_parts(&self, parts: &[&[u8]], mem: &GuestMemory) -> u32 {
        let mut segs = self.writable.iter();
        let mut cur = segs.next();
        let mut off = 0u32;
        let mut total = 0u32;

        for part in parts {
            let mut pos = 0usize;
            while pos < part.len() {
                let Some(s) = cur else {
                    return total;
                };
                let room = (s.len - off) as usize;
                if room == 0 {
                    cur = segs.next();
                    off = 0;
                    continue;
                }
                let n = room.min(part.len() - pos);
                mem.write(s.addr + off as u64, &part[pos..pos + n]).unwrap();
                pos += n;
                off += n as u32;
                total += n as u32;
            }
        }
        total
    }
}
