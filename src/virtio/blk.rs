use crate::virtio::{common, device::Device, virtq};
use applevisor::{error::Result, memory::Memory};
use num_enum::TryFromPrimitive;
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::FileExt,
};

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-3060001
const DEVICE_ID: u32 = 2;

const S_OK: u8 = 0;
const S_IOERR: u8 = 1;
const S_UNSUPP: u8 = 2;

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

#[derive(Debug)]
#[repr(C)]
struct RequestHeader {
    r#type: u32,
    reserver: u32,
    sector: u64,
}

impl RequestHeader {
    fn new(offset: u64, mem: &Memory) -> Result<RequestHeader> {
        Ok(RequestHeader {
            r#type: mem.read_u32(offset)?,
            reserver: mem.read_u32(offset + 4)?,
            sector: mem.read_u64(offset + 4 + 4)?,
        })
    }
}

pub struct Blk {
    host_disk_size: usize,
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
            host_disk_size,
            file,
        }
    }
}

impl Device for Blk {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::F_VERSION_1
    }

    fn config(&self) -> u32 {
        (self.host_disk_size / 512) as u32
    }

    fn num_queues(&self) -> u16 {
        1
    }

    fn handle_request(&mut self, queue: &virtq::Queue, head_idx: u16, mem: &mut Memory) -> u32 {
        let head_desc = queue.read_desc(head_idx, mem);
        let request_header = RequestHeader::new(head_desc.addr, mem).unwrap();
        let disk_offset = request_header.sector * 512;

        let req_type = RequestType::try_from(request_header.r#type).unwrap();

        let mut cur = head_desc.next();
        let mut written_len: u32 = 0;
        let mut status = S_OK;

        while let Some(cur_desc_idx) = cur {
            let cur_desc = queue.read_desc(cur_desc_idx, mem);

            let next = cur_desc.next();
            let is_status_desc = next.is_none();

            eprintln!(
                "req: type={:?}, sector={}, disk_offset={}, len={}",
                req_type, request_header.sector, disk_offset, cur_desc.len
            );

            if is_status_desc {
                if cur_desc.len < 1 || cur_desc.flags & virtq::flags::DESC_F_WRITE == 0 {
                    panic!("bad virtio-blk status descriptor: {:?}", cur_desc);
                }

                mem.write_u8(cur_desc.addr, status).unwrap();
                written_len += 1;
                break;
            }

            match req_type {
                RequestType::In => {
                    if !cur_desc.is_writable() {
                        status = S_IOERR;
                    } else if status == S_OK {
                        let mut buf = vec![0u8; cur_desc.len as usize];
                        self.file.read_at(&mut buf, disk_offset).unwrap();
                        mem.write(cur_desc.addr, &buf).unwrap();

                        written_len += cur_desc.len;
                    }
                }

                RequestType::Out => {
                    if cur_desc.is_writable() {
                        status = S_IOERR;
                    } else if status == S_OK {
                        let mut buf = vec![0u8; cur_desc.len as usize];
                        mem.read(cur_desc.addr, &mut buf).unwrap();
                        self.file.write_at(&buf, disk_offset).unwrap();
                    }
                }

                _ => {
                    status = S_UNSUPP;
                }
            }

            cur = next;
        }

        written_len
    }
}
