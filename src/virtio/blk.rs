use crate::virtio::{common, device::Device, virtq};
use applevisor::memory::Memory;
use num_enum::TryFromPrimitive;
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::FileExt,
};
use zerocopy::{FromBytes, Immutable};

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

#[derive(FromBytes, Immutable)]
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
    pub fn new(path: &str, host_disk_size: usize) -> Blk {
        assert_eq!(
            host_disk_size % 512,
            0,
            "disk size must be a multiple of 512"
        );

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .unwrap();

        let cur_len = file.metadata().unwrap().len();
        if cur_len < host_disk_size as u64 {
            file.set_len(host_disk_size as u64).unwrap();
        }

        Blk {
            sectors: (host_disk_size / 512) as u64,
            file,
        }
    }
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

    fn process_chain(
        &mut self,
        _q_idx: usize,
        queue: &virtq::Queue,
        head_idx: u16,
        mem: &mut Memory,
    ) -> Option<u32> {
        let chain = queue.collect_chain(head_idx, mem)?;

        if chain.readable.len() < size_of::<RequestHeader>() || chain.writable.is_empty() {
            return None;
        }

        let (header, _) = RequestHeader::read_from_prefix(&chain.readable).ok()?;
        let mut disk_offset = header.sector * 512;

        let (status_desc, data_writable) = chain.writable.split_last().unwrap();

        let mut written = 0u32;
        let mut status = status::OK;

        match RequestType::try_from(header.r#type) {
            Ok(RequestType::In) => {
                for d in data_writable {
                    let mut buf = vec![0u8; d.len as usize];
                    if self.file.read_at(&mut buf, disk_offset).is_err()
                        || mem.write(d.addr, &buf).is_err()
                    {
                        status = status::IOERR;
                        break;
                    }
                    disk_offset += d.len as u64;
                    written += d.len;
                }
            }

            Ok(RequestType::Out) => {
                let data = &chain.readable[size_of::<RequestHeader>()..];
                if self.file.write_at(data, disk_offset).is_err() {
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

        mem.write_u8(status_desc.addr, status).ok()?;
        Some(written + 1)
    }
}
