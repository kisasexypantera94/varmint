//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-1820002

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
