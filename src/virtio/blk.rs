use crate::virtio::{mmio, virtq};
use applevisor::memory::Memory;

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-3060001
const DEVICE_ID: u32 = 2;

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-7080006
const F_VERSION_1: u64 = 1 << 32;

const S_OK: u8 = 0;
const S_IOERR: u8 = 1;
const S_UNSUPP: u8 = 2;

pub struct Blk {
    host_disk_size: usize,

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
    pub fn new(host_disk_size: usize) -> Blk {
        assert_eq!(
            host_disk_size % 512,
            0,
            "disk size must be a multiple of 512"
        );

        Blk {
            host_disk_size,

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
            mmio::Reg::DeviceFeaturesSel => {
                self.device_features_sel = value;
            }
            mmio::Reg::DriverFeaturesSel => {
                self.driver_features_sel = value;
            }
            mmio::Reg::QueueNum => {
                self.queue_size = 256;
            }
            mmio::Reg::QueueSel => {
                self.queue_sel = value as u16;
            }
            mmio::Reg::QueueReady => {
                self.queue_ready = value;
            }
            mmio::Reg::QueueNotify => {
                const DESC_SIZE: u64 = size_of::<virtq::Desc>() as u64;
                const AVAIL_HEADER_SIZE: u64 = size_of::<virtq::AvailHeader>() as u64;
                const USED_HEADER_SIZE: u64 = size_of::<virtq::UsedHeader>() as u64;

                let desc_addr = virtq::queue_addr(self.queue_desc_lo, self.queue_desc_hi);
                let avail_addr = virtq::queue_addr(self.queue_driver_lo, self.queue_driver_hi);
                let used_addr = virtq::queue_addr(self.queue_device_lo, self.queue_device_hi);

                let avail_header = virtq::AvailHeader::new(avail_addr, mem).unwrap();
                eprintln!("AvailHeader: {:?}", avail_header);

                while avail_header.idx != self.last_avail_idx {
                    let avail_ring_idx = self.last_avail_idx % self.queue_size;
                    let avail_ring_addr =
                        avail_addr + AVAIL_HEADER_SIZE + (avail_ring_idx * 2) as u64;

                    let head = mem.read_u16(avail_ring_addr).unwrap();
                    let mut next = Some(head);

                    let mut has_used = false;
                    let mut written_len = 0;
                    while let Some(idx) = next {
                        let desc =
                            virtq::Desc::new(desc_addr + idx as u64 * DESC_SIZE, mem).unwrap();
                        eprintln!("Got descriptor: {:?}", desc);

                        written_len += desc.len;

                        has_used = true;

                        next = desc.next();
                    }

                    if has_used {
                        self.interrupt_status |= mmio::INT_VRING;

                        let used_ring_idx = self.last_used_idx % self.queue_size;
                        let used_ring_addr =
                            used_addr + USED_HEADER_SIZE + (used_ring_idx * 8) as u64;
                        mem.write_u32(used_ring_addr, head as u32).unwrap();
                        mem.write_u32(used_ring_addr + 4, written_len).unwrap();
                        self.last_used_idx = self.last_used_idx.wrapping_add(1);
                        mem.write_u16(used_addr + 2, self.last_used_idx).unwrap();
                    }

                    self.last_avail_idx = self.last_avail_idx.wrapping_add(1);
                }
            }
            mmio::Reg::InterruptAck => {
                self.interrupt_status &= !value;
            }
            mmio::Reg::Status => {
                self.status = value;
            }
            mmio::Reg::QueueDescLow => {
                self.queue_desc_lo = value;
            }
            mmio::Reg::QueueDescHigh => {
                self.queue_desc_hi = value;
            }
            mmio::Reg::QueueDriverLow => {
                self.queue_driver_lo = value;
            }
            mmio::Reg::QueueDriverHigh => {
                self.queue_driver_hi = value;
            }
            mmio::Reg::QueueDeviceLow => {
                self.queue_device_lo = value;
            }
            mmio::Reg::QueueDeviceHigh => {
                self.queue_device_hi = value;
            }
            _ => {}
        }
    }

    fn read_device_features(&self) -> u32 {
        let shift = self.device_features_sel * 32;
        (F_VERSION_1 >> shift) as u32
    }
}
