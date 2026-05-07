use crate::virtio::{mmio, virtq};
use applevisor::memory::Memory;

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-3060001
const DEVICE_ID: u32 = 2;

/// https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-7080006
const F_VERSION_1: u64 = 1 << 32;

pub struct Blk {
    host_disk_size: usize,

    device_features_sel: u32,
    driver_features_sel: u32,

    queue_sel: u16,
    queue_size: u16,
    queue_ready: u32,

    interrupt_ack: u32,
    status: u32,

    queue_desc_lo: u32,
    queue_desc_hi: u32,
    queue_driver_lo: u32,
    queue_driver_hi: u32,
    queue_device_lo: u32,
    queue_device_hi: u32,

    last_avail_idx: u16,
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

            interrupt_ack: 0,
            status: 0,

            queue_desc_lo: 0,
            queue_desc_hi: 0,
            queue_driver_lo: 0,
            queue_driver_hi: 0,
            queue_device_lo: 0,
            queue_device_hi: 0,

            last_avail_idx: 0,
        }
    }

    pub fn read(&mut self, offset: u64) -> u32 {
        match offset {
            mmio::reg::MAGIC_VALUE => mmio::MAGIC,
            mmio::reg::VERSION => mmio::VERSION,
            mmio::reg::DEVICE_ID => DEVICE_ID,
            mmio::reg::VENDOR_ID => mmio::VENDOR_ID,
            mmio::reg::DEVICE_FEATURES => self.read_device_features(),
            mmio::reg::QUEUE_NUM_MAX => 256,
            mmio::reg::QUEUE_NUM => 1,
            mmio::reg::QUEUE_READY => self.queue_ready,
            mmio::reg::STATUS => self.status,
            mmio::reg::CONFIG => (self.host_disk_size / 512) as u32,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u64, value: u32, mem: &mut Memory) {
        match offset {
            mmio::reg::DEVICE_FEATURES_SEL => {
                self.device_features_sel = value;
            }
            mmio::reg::DRIVER_FEATURES_SEL => {
                self.driver_features_sel = value;
            }
            mmio::reg::QUEUE_SEL => {
                self.queue_sel = value as u16;
            }
            mmio::reg::QUEUE_READY => {
                self.queue_ready = value;
            }
            mmio::reg::QUEUE_NOTIFY => {
                const DESC_SIZE: u64 = size_of::<virtq::Desc>() as u64;
                let avail_addr = virtq::queue_addr(self.queue_device_lo, self.queue_device_hi);
                let desc_addr = virtq::queue_addr(self.queue_desc_lo, self.queue_desc_hi);

                while let avail_header = virtq::AvailHeader::new(avail_addr, mem).unwrap()
                    && avail_header.idx != self.last_avail_idx
                {
                    let ring_idx = self.last_avail_idx % self.queue_size;
                    let ring_addr = self.last_avail_idx + ring_idx * 2;
                    let desc_idx = mem.read_u16(ring_addr as u64).unwrap() as u64;
                    let desc = virtq::Desc::new(desc_addr + desc_idx * DESC_SIZE, mem);
                    println!("Got descriptor: {:?}", desc);
                }

                todo!();
            }
            mmio::reg::STATUS => {
                self.status = value;
            }
            mmio::reg::QUEUE_DESC_LOW => {
                self.queue_desc_lo = value;
            }
            mmio::reg::QUEUE_DESC_HIGH => {
                self.queue_desc_hi = value;
            }
            mmio::reg::QUEUE_DRIVER_LOW => {
                self.queue_driver_lo = value;
            }
            mmio::reg::QUEUE_DRIVER_HIGH => {
                self.queue_driver_hi = value;
            }
            mmio::reg::QUEUE_DEVICE_LOW => {
                self.queue_device_lo = value;
            }
            mmio::reg::QUEUE_DEVICE_HIGH => {
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
