//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-1820002

use crate::{
    irq::IrqLine,
    memory::GuestMemory,
    virtio::{
        device::{self, Device, DeviceContext, ExternalEventHandler},
        virtq::{self, Queue},
    },
};
use num_enum::TryFromPrimitive;

pub const QUEUE_NOTIFY_OFFSET: u64 = 0x050;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u64)]
enum Reg {
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
    QueueNotify = QUEUE_NOTIFY_OFFSET,
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
    SHMSel = 0x0ac,
    SHMLenLow = 0x0b0,
    SHMLenHigh = 0x0b4,
    SHMBaseLow = 0x0b8,
    SHMBaseHigh = 0x0bc,
}

const CONFIG_BASE: u64 = 0x100;

/// Magic value at offset `reg::MAGIC_VALUE`. Little-endian "virt".
const MAGIC: u32 = 0x74726976;

/// MMIO transport version. 2 = modern (non-legacy).
const VERSION: u32 = 2;

/// "vmnt"
const VENDOR_ID: u32 = 0x76_6d_6e_74;

#[derive(Default, Clone)]
struct QueuePending {
    desc_lo: u32,
    desc_hi: u32,
    driver_lo: u32,
    driver_hi: u32,
    device_lo: u32,
    device_hi: u32,
    size: u16,
}

pub struct Transport<D: Device> {
    queues: Vec<virtq::Queue>,
    queues_pending: Vec<QueuePending>,
    device: D,

    device_features_sel: u32,
    driver_features_sel: u32,

    queue_sel: u16,

    shm_sel: u32,

    interrupt_status: u32,
    status: u32,
    irq: IrqLine,
}

impl<D: device::Device> Transport<D> {
    pub fn new(device: D, irq: IrqLine) -> Transport<D> {
        Transport {
            queues: vec![Queue::default(); device.num_queues() as usize],
            queues_pending: vec![QueuePending::default(); device.num_queues() as usize],
            device,
            device_features_sel: 0,
            driver_features_sel: 0,
            queue_sel: 0,
            shm_sel: 0,
            interrupt_status: 0,
            status: 0,
            irq,
        }
    }

    fn read_device_features(&self) -> u32 {
        let shift = self.device_features_sel * 32;
        (self.device.features() >> shift) as u32
    }

    pub fn read(&mut self, offset: u64, size: usize) -> u64 {
        assert!(matches!(size, 1 | 2 | 4 | 8));

        if offset >= CONFIG_BASE {
            let mut buf = [0u8; 8];
            let cfg_offset = offset - CONFIG_BASE;
            self.device.read_config(cfg_offset, &mut buf[..size]);
            return u64::from_le_bytes(buf);
        }

        let Ok(reg) = Reg::try_from(offset) else {
            return 0;
        };

        assert_eq!(size, 4, "virtio-mmio register access must be 32-bit");

        (match reg {
            Reg::MagicValue => MAGIC,
            Reg::Version => VERSION,
            Reg::DeviceId => self.device.id(),
            Reg::VendorId => VENDOR_ID,
            Reg::DeviceFeatures => self.read_device_features(),
            Reg::QueueNumMax => 256,
            Reg::QueueReady => self.queues[self.queue_sel as usize].ready as u32,
            Reg::InterruptStatus => self.interrupt_status,
            Reg::Status => self.status,
            Reg::SHMLenLow => match self.device.shared_memory_region(self.shm_sel) {
                Some(region) => region.len as u32,
                None => u32::MAX,
            },
            Reg::SHMLenHigh => match self.device.shared_memory_region(self.shm_sel) {
                Some(region) => (region.len >> 32) as u32,
                None => u32::MAX,
            },
            Reg::SHMBaseLow => match self.device.shared_memory_region(self.shm_sel) {
                Some(region) => region.base as u32,
                None => u32::MAX,
            },
            Reg::SHMBaseHigh => match self.device.shared_memory_region(self.shm_sel) {
                Some(region) => (region.base >> 32) as u32,
                None => u32::MAX,
            },
            _ => 0,
        }) as u64
    }

    fn current_pending_queue(&mut self) -> &mut QueuePending {
        &mut self.queues_pending[self.queue_sel as usize]
    }

    pub fn write(&mut self, offset: u64, size: usize, value: u64, mem: &GuestMemory) {
        assert!(matches!(size, 1 | 2 | 4 | 8));

        if offset >= CONFIG_BASE {
            let bytes = value.to_le_bytes();
            let cfg_offset = offset - CONFIG_BASE;
            self.device.write_config(cfg_offset, &bytes[..size]);
            return;
        }

        let Ok(reg) = Reg::try_from(offset) else {
            return;
        };

        assert_eq!(size, 4, "virtio-mmio register access must be 32-bit");
        let value = value as u32;

        match reg {
            Reg::DeviceFeaturesSel => self.device_features_sel = value,
            Reg::DriverFeaturesSel => self.driver_features_sel = value,
            Reg::QueueNum => self.current_pending_queue().size = value as u16,
            Reg::QueueSel => self.queue_sel = value as u16,
            Reg::QueueReady => {
                let q_idx = self.queue_sel as usize;
                if value == 1 {
                    let pending = self.current_pending_queue();

                    self.queues[q_idx] = Queue::new(
                        true,
                        pending.size,
                        queue_addr(pending.desc_lo, pending.desc_hi),
                        queue_addr(pending.driver_lo, pending.driver_hi),
                        queue_addr(pending.device_lo, pending.device_hi),
                    );
                } else {
                    self.queues[q_idx].ready = false;
                }
            }
            Reg::QueueNotify => {
                let q_idx = value as usize;
                if q_idx < self.queues.len() {
                    let mut ctx = DeviceContext::new(&mut self.queues, &mut self.interrupt_status, mem);
                    self.device.queue_notified(q_idx, &mut ctx);
                }
            }
            Reg::InterruptAck => self.interrupt_status &= !value,
            Reg::Status => {
                if value == 0 {
                    self.reset();
                } else {
                    self.status = value;
                }
            }
            Reg::QueueDescLow => self.current_pending_queue().desc_lo = value,
            Reg::QueueDescHigh => self.current_pending_queue().desc_hi = value,
            Reg::QueueDriverLow => self.current_pending_queue().driver_lo = value,
            Reg::QueueDriverHigh => self.current_pending_queue().driver_hi = value,
            Reg::QueueDeviceLow => self.current_pending_queue().device_lo = value,
            Reg::QueueDeviceHigh => self.current_pending_queue().device_hi = value,
            Reg::SHMSel => self.shm_sel = value,
            _ => {}
        }

        self.sync_irq();
    }

    fn sync_irq(&mut self) {
        self.irq.set(self.interrupt_status != 0);
    }

    fn reset(&mut self) {
        self.status = 0;
        self.interrupt_status = 0;

        self.device_features_sel = 0;
        self.driver_features_sel = 0;
        self.queue_sel = 0;

        for queue in &mut self.queues {
            queue.reset();
        }

        for pending in &mut self.queues_pending {
            *pending = QueuePending::default();
        }

        self.device.reset();
    }
}

pub fn queue_addr(lo: u32, hi: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

impl<D: ExternalEventHandler + Device> Transport<D> {
    pub fn handle_external_event(&mut self, event: D::Event<'_>, mem: &GuestMemory) {
        let mut ctx = DeviceContext::new(&mut self.queues, &mut self.interrupt_status, mem);
        self.device.on_event(event, &mut ctx);
        self.sync_irq();
    }
}
