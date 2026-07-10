pub const RAM_START: u64 = 0x40000000;
pub const RAM_SIZE: usize = 0x400000000;

pub const KERNEL_TEXT_OFFSET: u64 = 0x0;
pub const IMAGE_START: u64 = RAM_START + KERNEL_TEXT_OFFSET;
pub const INITRD_START: u64 = 0x48000000;
pub const DTB_START: u64 = 0x4F000000;
pub const GICD_START: u64 = 0x08000000;
pub const GICR_START: u64 = 0x080A0000;
pub const PSTATE_EL1H_DAIF_MASKED: u64 = 0x3c5;

pub const NUM_VCPUS: usize = 12;
pub const BOOT_VCPU_ID: usize = 0;
pub const FIRST_SECONDARY_VCPU_ID: usize = 1;

pub fn secondary_mpidr(vcpu_id: usize) -> u64 {
    vcpu_id as u64
}

pub fn secondary_index_for_mpidr(mpidr: u64) -> Option<usize> {
    let id = (mpidr & 0xff) as usize;
    if (FIRST_SECONDARY_VCPU_ID..NUM_VCPUS).contains(&id) {
        Some(id - FIRST_SECONDARY_VCPU_ID)
    } else {
        None
    }
}

pub const ESR_EC_HVC_AARCH64: u64 = 0x16;

pub const PSCI_VERSION: u64 = 0x84000000;
pub const PSCI_CPU_OFF: u64 = 0x84000002;
pub const PSCI_CPU_ON_64: u64 = 0xC4000003;
pub const PSCI_SYSTEM_OFF: u64 = 0x84000008;
pub const PSCI_SYSTEM_RESET: u64 = 0x84000009;
pub const PSCI_VERSION_0_2: u64 = 0x00000002;
pub const PSCI_SUCCESS: u64 = 0;
pub const PSCI_NOT_SUPPORTED: u64 = -1i64 as u64;
pub const PSCI_INVALID_PARAMETERS: u64 = -2i64 as u64;
pub const PSCI_DENIED: u64 = -3i64 as u64;
pub const PSCI_ALREADY_ON: u64 = -4i64 as u64;

pub const UART_START: u64 = 0x09000000;
pub const UART_SIZE: u64 = 0x1000;
pub const UART_SPI_OFFSET: u32 = 1;

pub const VIRTBLK_START: u64 = 0x0a000000;
pub const VIRTBLK_SIZE: u64 = 0x1000;
pub const VIRTBLK_SPI_OFFSET: u32 = 32;

pub const VIRTNET_START: u64 = 0x0a001000;
pub const VIRTNET_SIZE: u64 = 0x1000;
pub const VIRTNET_SPI_OFFSET: u32 = 33;

pub const VIRTGPU_START: u64 = 0x0a002000;
pub const VIRTGPU_SIZE: u64 = 0x1000;
pub const VIRTGPU_SPI_OFFSET: u32 = 34;

pub const VIRTIO_MMIO_QUEUE_NOTIFY: u64 = 0x50;

pub const VIRTINPUT_KEYBOARD_START: u64 = 0x0a003000;
pub const VIRTINPUT_KEYBOARD_SIZE: u64 = 0x1000;
pub const VIRTINPUT_KEYBOARD_SPI_OFFSET: u32 = 35;

pub const VIRTINPUT_TABLET_START: u64 = 0x0a004000;
pub const VIRTINPUT_TABLET_SIZE: u64 = 0x1000;
pub const VIRTINPUT_TABLET_SPI_OFFSET: u32 = 36;

pub const VIRTSND_START: u64 = 0x0a005000;
pub const VIRTSND_SIZE: u64 = 0x1000;
pub const VIRTSND_SPI_OFFSET: u32 = 37;

pub const VIRTCONSOLE_START: u64 = 0x0a006000;
pub const VIRTCONSOLE_SIZE: u64 = 0x1000;
pub const VIRTCONSOLE_SPI_OFFSET: u32 = 38;

pub const VIRTINPUT_MOUSE_START: u64 = 0x0a007000;
pub const VIRTINPUT_MOUSE_SIZE: u64 = 0x1000;
pub const VIRTINPUT_MOUSE_SPI_OFFSET: u32 = 39;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MmioDevice {
    Uart,
    VirtioBlk,
    VirtioNet,
    VirtioGpu,
    VirtioInputKeyboard,
    VirtioInputTablet,
    VirtioInputMouse,
    VirtioSnd,
    VirtioConsole,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DevicePlacement {
    Inline,
    ThreadOwned { owner: &'static str },
}

#[derive(Debug, Copy, Clone)]
pub struct MmioRoute {
    pub device: MmioDevice,
    pub offset: u64,
    pub placement: DevicePlacement,
}

struct DeviceThreadConfig {
    owner: &'static str,
    devices: &'static [MmioDevice],
}

pub const INLINE_MMIO_OWNER: &str = "vcpu-inline";
pub const GPU_MMIO_OWNER: &str = "gpu";

const DEVICE_THREAD_CONFIGS: &[DeviceThreadConfig] = &[
    DeviceThreadConfig {
        owner: INLINE_MMIO_OWNER,
        devices: &[
            MmioDevice::Uart,
            MmioDevice::VirtioBlk,
            MmioDevice::VirtioNet,
            MmioDevice::VirtioInputKeyboard,
            MmioDevice::VirtioInputTablet,
            MmioDevice::VirtioInputMouse,
            MmioDevice::VirtioSnd,
            MmioDevice::VirtioConsole,
        ],
    },
    DeviceThreadConfig {
        owner: GPU_MMIO_OWNER,
        devices: &[MmioDevice::VirtioGpu],
    },
];

fn device_placement(device: MmioDevice) -> DevicePlacement {
    DEVICE_THREAD_CONFIGS
        .iter()
        .find_map(|cfg| cfg.devices.contains(&device).then_some(cfg.owner))
        .map(|owner| {
            if owner == INLINE_MMIO_OWNER {
                DevicePlacement::Inline
            } else {
                DevicePlacement::ThreadOwned { owner }
            }
        })
        .unwrap_or(DevicePlacement::Inline)
}

pub fn classify(phys_addr: u64) -> Option<MmioRoute> {
    const REGIONS: &[(u64, u64, MmioDevice)] = &[
        (UART_START, UART_SIZE, MmioDevice::Uart),
        (VIRTBLK_START, VIRTBLK_SIZE, MmioDevice::VirtioBlk),
        (VIRTNET_START, VIRTNET_SIZE, MmioDevice::VirtioNet),
        (VIRTGPU_START, VIRTGPU_SIZE, MmioDevice::VirtioGpu),
        (
            VIRTINPUT_KEYBOARD_START,
            VIRTINPUT_KEYBOARD_SIZE,
            MmioDevice::VirtioInputKeyboard,
        ),
        (
            VIRTINPUT_TABLET_START,
            VIRTINPUT_TABLET_SIZE,
            MmioDevice::VirtioInputTablet,
        ),
        (
            VIRTINPUT_MOUSE_START,
            VIRTINPUT_MOUSE_SIZE,
            MmioDevice::VirtioInputMouse,
        ),
        (VIRTSND_START, VIRTSND_SIZE, MmioDevice::VirtioSnd),
        (VIRTCONSOLE_START, VIRTCONSOLE_SIZE, MmioDevice::VirtioConsole),
    ];

    REGIONS.iter().find_map(|&(base, size, device)| {
        (base..base + size).contains(&phys_addr).then(|| MmioRoute {
            device,
            offset: phys_addr - base,
            placement: device_placement(device),
        })
    })
}
