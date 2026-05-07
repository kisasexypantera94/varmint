//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-1820002

pub mod reg {
    pub const MAGIC_VALUE: u64 = 0x000;
    pub const VERSION: u64 = 0x004;
    pub const DEVICE_ID: u64 = 0x008;
    pub const VENDOR_ID: u64 = 0x00c;
    pub const DEVICE_FEATURES: u64 = 0x010;
    pub const DEVICE_FEATURES_SEL: u64 = 0x014;
    pub const DRIVER_FEATURES: u64 = 0x020;
    pub const DRIVER_FEATURES_SEL: u64 = 0x024;
    pub const QUEUE_SEL: u64 = 0x030;
    pub const QUEUE_NUM_MAX: u64 = 0x034;
    pub const QUEUE_NUM: u64 = 0x038;
    pub const QUEUE_READY: u64 = 0x044;
    pub const QUEUE_NOTIFY: u64 = 0x050;
    pub const INTERRUPT_STATUS: u64 = 0x060;
    pub const INTERRUPT_ACK: u64 = 0x064;
    pub const STATUS: u64 = 0x070;
    pub const QUEUE_DESC_LOW: u64 = 0x080;
    pub const QUEUE_DESC_HIGH: u64 = 0x084;
    pub const QUEUE_DRIVER_LOW: u64 = 0x090;
    pub const QUEUE_DRIVER_HIGH: u64 = 0x094;
    pub const QUEUE_DEVICE_LOW: u64 = 0x0a0;
    pub const QUEUE_DEVICE_HIGH: u64 = 0x0a4;
    pub const CONFIG_GENERATION: u64 = 0x0fc;
    pub const CONFIG: u64 = 0x100;
}

/// Magic value at offset `reg::MAGIC_VALUE`. Little-endian "virt".
pub const MAGIC: u32 = 0x74726976;

/// MMIO transport version. 2 = modern (non-legacy).
pub const VERSION: u32 = 2;

/// "vmnt"
pub const VENDOR_ID: u32 = 0x76_6d_6e_74;
