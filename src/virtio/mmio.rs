//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-1820002

use crate::virtio::{
    device,
    virtq::{self, VirtQueue},
};
use applevisor::memory::Memory;
use num_enum::TryFromPrimitive;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u64)]
pub enum Reg {
    MagicValue = 0x000,
    Version = 0x004,
    DeviceId = 0x008,
    VendorId = 0x00c,
    DeviceFeatures = 0x010,
    DeviceFeaturesSel = 0x014,
    DriverFeatures = 0x020,
    DriverFeaturesSel = 0x024,
    QueueSel = 0x030,
    QueueNumMax = 0x034,
    QueueNum = 0x038,
    QueueReady = 0x044,
    QueueNotify = 0x050,
    InterruptStatus = 0x060,
    InterruptAck = 0x064,
    Status = 0x070,
    QueueDescLow = 0x080,
    QueueDescHigh = 0x084,
    QueueDriverLow = 0x090,
    QueueDriverHigh = 0x094,
    QueueDeviceLow = 0x0a0,
    QueueDeviceHigh = 0x0a4,
    ConfigGeneration = 0x0fc,
    Config = 0x100,
}

/// Magic value at offset `reg::MAGIC_VALUE`. Little-endian "virt".
pub const MAGIC: u32 = 0x74726976;

/// MMIO transport version. 2 = modern (non-legacy).
pub const VERSION: u32 = 2;

/// "vmnt"
pub const VENDOR_ID: u32 = 0x76_6d_6e_74;

pub const INT_VRING: u32 = 1 << 0;

pub const INT_CONFIG: u32 = 1 << 1;

pub struct Transport<D: device::Device> {
    queues: Vec<virtq::VirtQueue>,
    device: D,

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
}

impl<D: device::Device> Transport<D> {
    pub fn new(device: D) -> Transport<D> {
        Transport {
            queues: vec![VirtQueue::default(); device.num_queues() as usize],
            device,
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
        }
    }

    fn read_device_features(&self) -> u32 {
        let shift = self.device_features_sel * 32;
        (self.device.features() >> shift) as u32
    }

    pub fn is_asserted(&mut self) -> bool {
        return self.interrupt_status != 0;
    }

    pub fn read(&mut self, offset: u64) -> u32 {
        let Ok(reg) = Reg::try_from(offset) else {
            return 0;
        };

        eprintln!("virtio: read, reg={:?}, offset={}", reg, offset);

        match reg {
            Reg::MagicValue => MAGIC,
            Reg::Version => VERSION,
            Reg::DeviceId => self.device.id(),
            Reg::VendorId => VENDOR_ID,
            Reg::DeviceFeatures => self.read_device_features(),
            Reg::QueueNumMax => 256,
            Reg::QueueReady => self.queue_ready,
            Reg::InterruptStatus => self.interrupt_status,
            Reg::Status => self.status,
            Reg::Config => self.device.config(),
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u64, value: u32, mem: &mut Memory) {
        let Ok(reg) = Reg::try_from(offset) else {
            return;
        };

        eprintln!(
            "virtio: write, reg={:?}, value={}, offset={}",
            reg, value, offset
        );

        match reg {
            Reg::DeviceFeaturesSel => self.device_features_sel = value,
            Reg::DriverFeaturesSel => self.driver_features_sel = value,
            Reg::QueueNum => self.queue_size = value as u16,
            Reg::QueueSel => self.queue_sel = value as u16,
            Reg::QueueReady => {
                if value == 1 {
                    self.queues[self.queue_sel as usize] = VirtQueue::new(
                        self.queue_size,
                        queue_addr(self.queue_desc_lo, self.queue_desc_hi),
                        queue_addr(self.queue_driver_lo, self.queue_driver_hi),
                        queue_addr(self.queue_device_lo, self.queue_device_hi),
                    );
                }
            }
            Reg::QueueNotify => {
                let q = &mut self.queues[value as usize];

                let raised = self.device.process_queue(q, mem);

                if raised {
                    self.interrupt_status |= INT_VRING;
                }
            }
            Reg::InterruptAck => self.interrupt_status &= !value,
            Reg::Status => self.status = value,
            Reg::QueueDescLow => self.queue_desc_lo = value,
            Reg::QueueDescHigh => self.queue_desc_hi = value,
            Reg::QueueDriverLow => self.queue_driver_lo = value,
            Reg::QueueDriverHigh => self.queue_driver_hi = value,
            Reg::QueueDeviceLow => self.queue_device_lo = value,
            Reg::QueueDeviceHigh => self.queue_device_hi = value,
            _ => {}
        }
    }
}

pub fn queue_addr(lo: u32, hi: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}
