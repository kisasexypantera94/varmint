use crate::{
    memory::GuestMemory,
    virtio::{
        chain::ChainData,
        common,
        device::{ChainAction, Device, DeviceContext},
    },
};
use num_enum::TryFromPrimitive;
use std::{
    fs::{File, OpenOptions},
    io,
    os::{fd::AsRawFd, unix::fs::FileExt},
    path::Path,
};
use zerocopy::{FromBytes, Immutable, IntoBytes};

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-3060001
const DEVICE_ID: u32 = 2;

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

pub struct Blk {
    sectors: u64,
    file: File,
}

impl Blk {
    pub fn new(path: &Path) -> io::Result<Blk> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        lock_disk(&file, path)?;

        let size = file.metadata()?.len();
        if size == 0 || size % 512 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "disk {} has invalid size {size}; expected a non-empty multiple of 512 bytes",
                    path.display()
                ),
            ));
        }

        Ok(Blk {
            sectors: size / 512,
            file,
        })
    }
}

fn lock_disk(file: &File, path: &Path) -> io::Result<()> {
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        return Err(io::Error::new(
            error.kind(),
            format!("disk {} is already in use by another Varmint process", path.display()),
        ));
    }

    Err(io::Error::new(
        error.kind(),
        format!("failed to lock disk {} exclusively: {error}", path.display()),
    ))
}

impl Device for Blk {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let cfg = self.sectors.to_le_bytes();
        let offset = offset as usize;
        data.copy_from_slice(&cfg[offset..offset + data.len()]);
    }

    fn num_queues(&self) -> u16 {
        1
    }

    fn queue_notified(&mut self, queue_idx: usize, ctx: &mut DeviceContext<'_>) {
        while let Some(chain) = ctx.pop_chain(queue_idx) {
            match self.process_chain(&chain.data, ctx.mem()) {
                ChainAction::Complete(written) => ctx.complete(chain.token, written),
                ChainAction::Deferred => {}
            }
        }
    }
}

impl Blk {
    fn process_chain(&mut self, chain: &ChainData, mem: &GuestMemory) -> ChainAction {
        const HEADER_LEN: usize = size_of::<RequestHeader>();

        let Some((status_seg, data_writable)) = chain.writable.split_last() else {
            return ChainAction::Complete(0);
        };

        let mut written = 0u32;
        let mut status = status::OK;

        match chain.read_obj::<RequestHeader>(0, mem) {
            None => status = status::IOERR,
            Some(header) => {
                let mut disk_offset = header.sector * 512;

                match RequestType::try_from(header.r#type) {
                    Ok(RequestType::In) => {
                        for d in data_writable {
                            let mut buf = vec![0u8; d.len as usize];
                            if self.file.read_at(&mut buf, disk_offset).is_err() || mem.write(d.addr, &buf).is_err() {
                                status = status::IOERR;
                                break;
                            }
                            disk_offset += d.len as u64;
                            written += d.len;
                        }
                    }

                    Ok(RequestType::Out) => {
                        let mut data = vec![0u8; chain.readable_len() - HEADER_LEN];
                        if chain.read_at(HEADER_LEN, &mut data, mem).is_none()
                            || self.file.write_at(&data, disk_offset).is_err()
                        {
                            status = status::IOERR;
                        }
                    }

                    Ok(RequestType::Flush) => {
                        if self.file.sync_all().is_err() {
                            status = status::IOERR;
                        }
                    }

                    _ => status = status::UNSUPP,
                }
            }
        }

        if mem.write_u8(status_seg.addr, status).is_err() {
            return ChainAction::Complete(0);
        }

        ChainAction::Complete(written + 1)
    }
}
