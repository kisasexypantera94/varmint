use crate::{
    memory::GuestMemory,
    virtio::{
        chain::{ChainData, Seg},
        common,
        device::{ChainAction, ChainToken, Device, DeviceContext, ExternalEventHandler},
    },
};
use num_enum::TryFromPrimitive;
use std::{
    fs::{File, OpenOptions},
    io,
    os::{fd::AsRawFd, unix::fs::FileExt},
    path::Path,
    sync::mpsc::{self, Sender},
    thread,
};
use zerocopy::{FromBytes, Immutable, IntoBytes};

const DEVICE_ID: u32 = 2;
const SECTOR_SIZE: u64 = 512;
const MAX_IO_BYTES: usize = 4 * 1024 * 1024;
const MAX_IN_FLIGHT: usize = 128;
const MAX_DISCARD_SECTORS: u32 = 16 * 1024;
const F_FLUSH: u64 = 1 << 9;
const F_DISCARD: u64 = 1 << 13;

mod status {
    pub const OK: u8 = 0;
    pub const IOERR: u8 = 1;
    pub const UNSUPP: u8 = 2;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
enum RequestType {
    In = 0,
    Out = 1,
    Flush = 4,
    GetId = 8,
    GetLifetime = 10,
    Discard = 11,
    WriteZeroes = 13,
    SecureErase = 14,
}

#[derive(IntoBytes, FromBytes, Immutable)]
#[repr(C)]
struct RequestHeader {
    r#type: u32,
    reserved: u32,
    sector: u64,
}

#[derive(IntoBytes, FromBytes, Immutable)]
#[repr(C)]
struct DiscardRange {
    sector: u64,
    num_sectors: u32,
    flags: u32,
}

enum IoOp {
    Read { offset: u64, len: usize },
    Write { offset: u64, data: Vec<u8> },
    Flush,
    Discard { offset: u64, len: u64 },
}

struct Pending {
    token: ChainToken,
    status_addr: u64,
    read_segments: Vec<Seg>,
}

struct IoRequest {
    generation: u64,
    pending: Pending,
    op: IoOp,
}

pub struct IoCompletion {
    generation: u64,
    pending: Pending,
    result: io::Result<Option<Vec<u8>>>,
}

pub struct Blk {
    sectors: u64,
    discard_alignment: u32,
    generation: u64,
    in_flight: usize,
    io_tx: Sender<IoRequest>,
}

impl Blk {
    pub fn new(path: &Path, on_complete: impl Fn(IoCompletion) + Send + 'static) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        lock_disk(&file, path)?;

        let size = file.metadata()?.len();
        if size == 0 || size % SECTOR_SIZE != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid disk size"));
        }

        let discard_alignment = apfs_discard_alignment(&file)?;
        let (io_tx, io_rx) = mpsc::channel::<IoRequest>();
        thread::Builder::new().name("varmint-blk-io".into()).spawn(move || {
            while let Ok(request) = io_rx.recv() {
                on_complete(IoCompletion {
                    generation: request.generation,
                    pending: request.pending,
                    result: execute(&file, request.op),
                });
            }
        })?;

        Ok(Self {
            sectors: size / SECTOR_SIZE,
            discard_alignment,
            generation: 0,
            in_flight: 0,
            io_tx,
        })
    }

    fn respond(mem: &GuestMemory, addr: u64, status: u8) -> ChainAction {
        ChainAction::Complete(if mem.write_u8(addr, status).is_ok() { 1 } else { 0 })
    }

    fn make_op(
        &self,
        header: RequestHeader,
        chain: &ChainData,
        writable_data: &[Seg],
        mem: &GuestMemory,
    ) -> Result<IoOp, u8> {
        const HEADER_LEN: usize = size_of::<RequestHeader>();

        match RequestType::try_from(header.r#type) {
            Ok(RequestType::In) => {
                if chain.readable_len() != HEADER_LEN {
                    return Err(status::IOERR);
                }
                let len = writable_data
                    .iter()
                    .try_fold(0usize, |sum, seg| sum.checked_add(seg.len as usize))
                    .filter(|len| *len <= MAX_IO_BYTES && *len % SECTOR_SIZE as usize == 0)
                    .ok_or(status::IOERR)?;
                let offset = checked_offset(self.sectors, header.sector, len as u64).ok_or(status::IOERR)?;
                Ok(IoOp::Read { offset, len })
            }
            Ok(RequestType::Out) => {
                if !writable_data.is_empty() {
                    return Err(status::IOERR);
                }
                let len = chain.readable_len().checked_sub(HEADER_LEN).ok_or(status::IOERR)?;
                if len > MAX_IO_BYTES || len % SECTOR_SIZE as usize != 0 {
                    return Err(status::IOERR);
                }
                let offset = checked_offset(self.sectors, header.sector, len as u64).ok_or(status::IOERR)?;
                let mut data = vec![0; len];
                chain.read_at(HEADER_LEN, &mut data, mem).ok_or(status::IOERR)?;
                Ok(IoOp::Write { offset, data })
            }
            Ok(RequestType::Flush) => {
                if header.sector != 0 || !writable_data.is_empty() || chain.readable_len() != HEADER_LEN {
                    return Err(status::IOERR);
                }
                Ok(IoOp::Flush)
            }
            Ok(RequestType::Discard) => {
                if self.discard_alignment == 0 {
                    return Err(status::UNSUPP);
                }
                if !writable_data.is_empty() || chain.readable_len() != HEADER_LEN + size_of::<DiscardRange>() {
                    return Err(status::IOERR);
                }

                let range = chain.read_obj::<DiscardRange>(HEADER_LEN, mem).ok_or(status::IOERR)?;
                if range.flags != 0 || range.num_sectors == 0 || range.num_sectors > MAX_DISCARD_SECTORS {
                    return Err(status::IOERR);
                }

                let alignment = u64::from(self.discard_alignment);
                if range.sector % alignment != 0 || u64::from(range.num_sectors) % alignment != 0 {
                    return Err(status::IOERR);
                }

                let len = u64::from(range.num_sectors) * SECTOR_SIZE;
                let offset = checked_offset(self.sectors, range.sector, len).ok_or(status::IOERR)?;
                Ok(IoOp::Discard { offset, len })
            }
            _ => Err(status::UNSUPP),
        }
    }

    fn submit(&mut self, chain: &ChainData, token: ChainToken, mem: &GuestMemory) -> ChainAction {
        let Some((status_seg, writable_data)) = chain.writable.split_last() else {
            return ChainAction::Complete(0);
        };
        if status_seg.len == 0 {
            return ChainAction::Complete(0);
        }

        let Some(header) = chain.read_obj::<RequestHeader>(0, mem) else {
            return Self::respond(mem, status_seg.addr, status::IOERR);
        };
        let op = match self.make_op(header, chain, writable_data, mem) {
            Ok(op) => op,
            Err(status) => return Self::respond(mem, status_seg.addr, status),
        };

        let pending = Pending {
            token,
            status_addr: status_seg.addr,
            read_segments: if matches!(op, IoOp::Read { .. }) {
                writable_data.to_vec()
            } else {
                Vec::new()
            },
        };

        if self
            .io_tx
            .send(IoRequest {
                generation: self.generation,
                pending,
                op,
            })
            .is_err()
        {
            return Self::respond(mem, status_seg.addr, status::IOERR);
        }

        self.in_flight += 1;
        ChainAction::Deferred
    }
}

fn checked_offset(sectors: u64, sector: u64, len: u64) -> Option<u64> {
    let offset = sector.checked_mul(SECTOR_SIZE)?;
    (offset.checked_add(len)? <= sectors.checked_mul(SECTOR_SIZE)?).then_some(offset)
}

fn execute(file: &File, op: IoOp) -> io::Result<Option<Vec<u8>>> {
    match op {
        IoOp::Read { offset, len } => {
            let mut data = vec![0; len];
            read_exact_at(file, &mut data, offset)?;
            Ok(Some(data))
        }
        IoOp::Write { offset, data } => {
            write_all_at(file, &data, offset)?;
            Ok(None)
        }
        IoOp::Flush => {
            file.sync_all()?;
            Ok(None)
        }
        IoOp::Discard { offset, len } => {
            punch_hole(file, offset, len)?;
            Ok(None)
        }
    }
}

fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = file.read_at(buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        offset += n as u64;
        buf = &mut buf[n..];
    }
    Ok(())
}

fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = file.write_at(buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        offset += n as u64;
        buf = &buf[n..];
    }
    Ok(())
}

fn apfs_discard_alignment(file: &File) -> io::Result<u32> {
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let stat = unsafe { stat.assume_init() };
    let kind: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    if kind != b"apfs" || stat.f_bsize < 512 || stat.f_bsize % 512 != 0 {
        return Ok(0);
    }
    Ok(stat.f_bsize / 512)
}

fn punch_hole(file: &File, offset: u64, len: u64) -> io::Result<()> {
    let mut hole = libc::fpunchhole_t {
        fp_flags: 0,
        reserved: 0,
        fp_offset: i64::try_from(offset).map_err(|_| io::ErrorKind::InvalidInput)?,
        fp_length: i64::try_from(len).map_err(|_| io::ErrorKind::InvalidInput)?,
    };
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_PUNCHHOLE, &mut hole) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn lock_disk(file: &File, path: &Path) -> io::Result<()> {
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    Err(io::Error::new(
        error.kind(),
        format!("disk {} cannot be locked: {error}", path.display()),
    ))
}

impl Device for Blk {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1 | F_FLUSH | if self.discard_alignment != 0 { F_DISCARD } else { 0 }
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let mut config = [0u8; 48];
        config[..8].copy_from_slice(&self.sectors.to_le_bytes());
        if self.discard_alignment != 0 {
            config[36..40].copy_from_slice(&MAX_DISCARD_SECTORS.to_le_bytes());
            config[40..44].copy_from_slice(&1u32.to_le_bytes());
            config[44..48].copy_from_slice(&self.discard_alignment.to_le_bytes());
        }

        data.fill(0);
        let Ok(start) = usize::try_from(offset) else {
            return;
        };
        let Some(available) = config.len().checked_sub(start) else {
            return;
        };
        let len = data.len().min(available);
        data[..len].copy_from_slice(&config[start..start + len]);
    }

    fn num_queues(&self) -> u16 {
        1
    }

    fn queue_notified(&mut self, queue_idx: usize, ctx: &mut DeviceContext<'_>) {
        while self.in_flight < MAX_IN_FLIGHT {
            let Some(chain) = ctx.pop_chain(queue_idx) else {
                break;
            };
            if let ChainAction::Complete(written) = self.submit(&chain.data, chain.token, ctx.mem()) {
                ctx.complete(chain.token, written);
            }
        }
    }

    fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.in_flight = 0;
    }
}

impl ExternalEventHandler for Blk {
    type Event<'a> = &'a IoCompletion;

    fn on_event(&mut self, event: &IoCompletion, ctx: &mut DeviceContext<'_>) {
        if event.generation != self.generation {
            return;
        }
        if self.in_flight == 0 {
            eprintln!("virtio-blk: unexpected completion with no request in flight");
            return;
        }
        self.in_flight -= 1;

        let pending = &event.pending;
        let mut written = 0u32;
        let mut status = status::OK;

        match &event.result {
            Ok(Some(data)) => {
                let mut offset = 0;
                for seg in &pending.read_segments {
                    let end = offset + seg.len as usize;
                    if ctx.mem().write(seg.addr, &data[offset..end]).is_err() {
                        status = status::IOERR;
                        break;
                    }
                    offset = end;
                    written += seg.len;
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("virtio-blk: request failed: {error}");
                status = status::IOERR;
            }
        }

        if ctx.mem().write_u8(pending.status_addr, status).is_ok() {
            written += 1;
        } else {
            written = 0;
        }

        ctx.complete(pending.token, written);
        self.queue_notified(0, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds() {
        assert_eq!(checked_offset(10, 9, 512), Some(4608));
        assert_eq!(checked_offset(10, 9, 1024), None);
        assert_eq!(checked_offset(10, u64::MAX, 512), None);
    }

    #[test]
    fn full_io() {
        let name = format!(
            "varmint-blk-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        );
        let path = std::env::temp_dir().join(name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let data = vec![0xab; 64 * 1024];
        write_all_at(&file, &data, 512).unwrap();
        let mut got = vec![0; data.len()];
        read_exact_at(&file, &mut got, 512).unwrap();
        assert_eq!(got, data);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
