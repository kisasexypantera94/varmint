use crate::virtio::{mmio, virtq};
use applevisor::{error::Result, memory::Memory};
use num_enum::TryFromPrimitive;
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::FileExt,
};

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-3060001
const DEVICE_ID: u32 = 2;

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-7080006
const F_VERSION_1: u64 = 1 << 32;

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

    device_features_sel: u32,
    driver_features_sel: u32,

    queue_sel: u16,
    queue_size: u16,
    queue_ready: u32,

    interrupt_status: u32,
    status: u32,

    queue_desc_lo: u32,
    queue_desc_hi: u32,
    queue_driver_lo: u32,
    queue_driver_hi: u32,
    queue_device_lo: u32,
    queue_device_hi: u32,

    last_avail_idx: u16,
    last_used_idx: u16,
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

            device_features_sel: 0,
            driver_features_sel: 0,

            queue_sel: 0,
            queue_size: 0,
            queue_ready: 0,

            interrupt_status: 0,
            status: 0,

            queue_desc_lo: 0,
            queue_desc_hi: 0,
            queue_driver_lo: 0,
            queue_driver_hi: 0,
            queue_device_lo: 0,
            queue_device_hi: 0,

            last_avail_idx: 0,
            last_used_idx: 0,
        }
    }

    pub fn is_asserted(&mut self) -> bool {
        return self.interrupt_status != 0;
    }

    pub fn read(&mut self, offset: u64) -> u32 {
        let Ok(reg) = mmio::Reg::try_from(offset) else {
            return 0;
        };

        eprintln!("Blk access: read, reg={:?}, offset={}", reg, offset);

        match reg {
            mmio::Reg::MagicValue => mmio::MAGIC,
            mmio::Reg::Version => mmio::VERSION,
            mmio::Reg::DeviceId => DEVICE_ID,
            mmio::Reg::VendorId => mmio::VENDOR_ID,
            mmio::Reg::DeviceFeatures => self.read_device_features(),
            mmio::Reg::QueueNumMax => 256,
            mmio::Reg::QueueReady => self.queue_ready,
            mmio::Reg::InterruptStatus => self.interrupt_status,
            mmio::Reg::Status => self.status,
            mmio::Reg::Config => (self.host_disk_size / 512) as u32,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u64, value: u32, mem: &mut Memory) {
        let Ok(reg) = mmio::Reg::try_from(offset) else {
            return;
        };

        eprintln!(
            "Blk access: write, reg={:?}, value={}, offset={}",
            reg, value, offset
        );

        match reg {
            mmio::Reg::DeviceFeaturesSel => self.device_features_sel = value,
            mmio::Reg::DriverFeaturesSel => self.driver_features_sel = value,
            mmio::Reg::QueueNum => self.queue_size = 256,
            mmio::Reg::QueueSel => self.queue_sel = value as u16,
            mmio::Reg::QueueReady => self.queue_ready = value,
            mmio::Reg::QueueNotify => self.process_queue_notify(value, mem),
            mmio::Reg::InterruptAck => self.interrupt_status &= !value,
            mmio::Reg::Status => self.status = value,
            mmio::Reg::QueueDescLow => self.queue_desc_lo = value,
            mmio::Reg::QueueDescHigh => self.queue_desc_hi = value,
            mmio::Reg::QueueDriverLow => self.queue_driver_lo = value,
            mmio::Reg::QueueDriverHigh => self.queue_driver_hi = value,
            mmio::Reg::QueueDeviceLow => self.queue_device_lo = value,
            mmio::Reg::QueueDeviceHigh => self.queue_device_hi = value,
            _ => {}
        }
    }

    fn process_queue_notify(&mut self, value: u32, mem: &mut Memory) {
        const AVAIL_HEADER_SIZE: u64 = size_of::<virtq::AvailHeader>() as u64;

        let avail_addr = virtq::queue_addr(self.queue_driver_lo, self.queue_driver_hi);

        let avail_header = virtq::AvailHeader::new(avail_addr, mem).unwrap();
        eprintln!("AvailHeader: {:?}", avail_header);

        while avail_header.idx != self.last_avail_idx {
            let avail_ring_idx = self.last_avail_idx % self.queue_size;
            let avail_ring_addr = avail_addr + AVAIL_HEADER_SIZE + (avail_ring_idx * 2) as u64;

            let head_idx = mem.read_u16(avail_ring_addr).unwrap() as u64;

            self.process_desc_chain(head_idx, mem);

            self.last_avail_idx = self.last_avail_idx.wrapping_add(1);
        }
    }

    fn process_desc_chain(&mut self, head_idx: u64, mem: &mut Memory) {
        const DESC_SIZE: u64 = size_of::<virtq::Desc>() as u64;
        const USED_HEADER_SIZE: u64 = size_of::<virtq::UsedHeader>() as u64;

        let desc_addr = virtq::queue_addr(self.queue_desc_lo, self.queue_desc_hi);
        let used_addr = virtq::queue_addr(self.queue_device_lo, self.queue_device_hi);

        let head_desc = virtq::Desc::new(desc_addr + head_idx * DESC_SIZE, mem).unwrap();
        let request_header = RequestHeader::new(head_desc.addr, mem).unwrap();
        let disk_offset = request_header.sector * 512;

        let req_type = RequestType::try_from(request_header.r#type).unwrap();

        let mut cur = head_desc.next();
        let mut written_len: u32 = 0;
        let mut status = S_OK;
        let mut seen = 1;

        while let Some(cur_desc_idx) = cur {
            assert!(
                cur_desc_idx < self.queue_size,
                "bad descriptor index: {}",
                cur_desc_idx
            );

            assert!(seen < self.queue_size, "descriptor chain loop?",);

            let cur_desc =
                virtq::Desc::new(desc_addr + cur_desc_idx as u64 * DESC_SIZE, mem).unwrap();

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
            seen += 1;
        }

        if seen > 0 {
            let used_ring_idx = self.last_used_idx % self.queue_size;
            let used_ring_addr = used_addr + USED_HEADER_SIZE + (used_ring_idx * 8) as u64;
            mem.write_u32(used_ring_addr, head_idx as u32).unwrap();
            mem.write_u32(used_ring_addr + 4, written_len).unwrap();
            self.last_used_idx = self.last_used_idx.wrapping_add(1);
            mem.write_u16(used_addr + 2, self.last_used_idx).unwrap();

            self.interrupt_status |= mmio::INT_VRING;
        }
    }

    fn read_device_features(&self) -> u32 {
        let shift = self.device_features_sel * 32;
        (F_VERSION_1 >> shift) as u32
    }
}
