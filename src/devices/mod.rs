mod events;
mod gpu;

use crate::{
    audio, clipboard,
    cpu::CpuRuntime,
    display::{DisplayBuffer, DisplayEvent},
    irq,
    machine::*,
    memory::GuestMemory,
    net, stdio, uart, virtio,
};
use applevisor::prelude::*;
pub use events::{RuntimeEvent, RuntimeInputEvent};
use std::{
    sync::{
        Mutex,
        mpsc::{Receiver, Sender},
    },
    thread,
};
use winit::event_loop::EventLoopProxy;

pub struct HostBackends {
    pub disk: virtio::Blk,
    pub net: net::Backend,
    pub audio: audio::PeriodSink,
    pub clipboard: clipboard::Sink,
    pub serial: stdio::Sink,
}

pub struct Devices {
    uart: Mutex<uart::Uart>,
    blk: Mutex<virtio::MmioTransport<virtio::Blk>>,
    net: Mutex<virtio::MmioTransport<virtio::Net>>,
    gpu: gpu::Handle,
    keyboard: Mutex<virtio::MmioTransport<virtio::Input>>,
    tablet: Mutex<virtio::MmioTransport<virtio::Input>>,
    mouse: Mutex<virtio::MmioTransport<virtio::Input>>,
    snd: Mutex<virtio::MmioTransport<virtio::Snd>>,
    console: Mutex<virtio::MmioTransport<virtio::Console>>,
}

pub struct Runtime<'a> {
    vm: &'a VirtualMachineInstance<GicEnabled>,
    devices: Devices,
    cpus: CpuRuntime,
    gpu_worker: gpu::Worker,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum MmioDevice {
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

#[derive(Debug, Copy, Clone)]
struct MmioRoute {
    device: MmioDevice,
    offset: u64,
}

struct MmioRegion {
    base: u64,
    size: u64,
    device: MmioDevice,
}

const MMIO_REGIONS: &[MmioRegion] = &[
    MmioRegion {
        base: UART_START,
        size: UART_SIZE,
        device: MmioDevice::Uart,
    },
    MmioRegion {
        base: VIRTBLK_START,
        size: VIRTBLK_SIZE,
        device: MmioDevice::VirtioBlk,
    },
    MmioRegion {
        base: VIRTNET_START,
        size: VIRTNET_SIZE,
        device: MmioDevice::VirtioNet,
    },
    MmioRegion {
        base: VIRTGPU_START,
        size: VIRTGPU_SIZE,
        device: MmioDevice::VirtioGpu,
    },
    MmioRegion {
        base: VIRTINPUT_KEYBOARD_START,
        size: VIRTINPUT_KEYBOARD_SIZE,
        device: MmioDevice::VirtioInputKeyboard,
    },
    MmioRegion {
        base: VIRTINPUT_TABLET_START,
        size: VIRTINPUT_TABLET_SIZE,
        device: MmioDevice::VirtioInputTablet,
    },
    MmioRegion {
        base: VIRTINPUT_MOUSE_START,
        size: VIRTINPUT_MOUSE_SIZE,
        device: MmioDevice::VirtioInputMouse,
    },
    MmioRegion {
        base: VIRTSND_START,
        size: VIRTSND_SIZE,
        device: MmioDevice::VirtioSnd,
    },
    MmioRegion {
        base: VIRTCONSOLE_START,
        size: VIRTCONSOLE_SIZE,
        device: MmioDevice::VirtioConsole,
    },
];

impl<'a> Runtime<'a> {
    pub fn new(
        vm: &'a VirtualMachineInstance<GicEnabled>,
        runtime_event_tx: Sender<RuntimeEvent>,
        backends: HostBackends,
        vcpus: usize,
    ) -> Result<Self> {
        let (spi_int_start, _) = GicConfig::get_spi_interrupt_range()?;

        let uart = Mutex::new(uart::Uart::new(
            irq::IrqLine::new(vm, spi_int_start + UART_SPI_OFFSET),
            backends.serial,
        ));

        let blk = Mutex::new(virtio::MmioTransport::new(
            backends.disk,
            irq::IrqLine::new(vm, spi_int_start + VIRTBLK_SPI_OFFSET),
        ));

        let net = Mutex::new(virtio::MmioTransport::new(
            virtio::Net::new(backends.net.mac(), backends.net),
            irq::IrqLine::new(vm, spi_int_start + VIRTNET_SPI_OFFSET),
        ));

        let (gpu, gpu_worker) = gpu::channel(irq::IrqLine::new(vm, spi_int_start + VIRTGPU_SPI_OFFSET));

        let keyboard = Mutex::new(virtio::MmioTransport::new(
            virtio::Input::keyboard(),
            irq::IrqLine::new(vm, spi_int_start + VIRTINPUT_KEYBOARD_SPI_OFFSET),
        ));

        let tablet = Mutex::new(virtio::MmioTransport::new(
            virtio::Input::tablet(),
            irq::IrqLine::new(vm, spi_int_start + VIRTINPUT_TABLET_SPI_OFFSET),
        ));

        let mouse = Mutex::new(virtio::MmioTransport::new(
            virtio::Input::mouse(),
            irq::IrqLine::new(vm, spi_int_start + VIRTINPUT_MOUSE_SPI_OFFSET),
        ));

        let snd = Mutex::new(virtio::MmioTransport::new(
            virtio::Snd::new(backends.audio),
            irq::IrqLine::new(vm, spi_int_start + VIRTSND_SPI_OFFSET),
        ));

        let console = Mutex::new(virtio::MmioTransport::new(
            virtio::Console::new(backends.clipboard),
            irq::IrqLine::new(vm, spi_int_start + VIRTCONSOLE_SPI_OFFSET),
        ));

        Ok(Self {
            vm,
            devices: Devices {
                uart,
                blk,
                net,
                gpu,
                keyboard,
                tablet,
                mouse,
                snd,
                console,
            },
            cpus: CpuRuntime::new(vm, IMAGE_START, DTB_START, vcpus)?,
            gpu_worker,
        })
    }

    pub fn run(
        self,
        mem: &GuestMemory,
        display: &Mutex<DisplayBuffer>,
        runtime_event_rx: Receiver<RuntimeEvent>,
        audio_event_rx: Receiver<audio::BackendEvent>,
        display_proxy: &EventLoopProxy<DisplayEvent>,
    ) -> Result<()> {
        thread::scope(|scope| -> Result<()> {
            scope.spawn(|| self.gpu_worker.run(mem, display, display_proxy));

            scope.spawn(|| {
                let runtime_events = events::RuntimeEventPump::new(mem, &self.devices, runtime_event_rx);
                runtime_events.run();
            });

            scope.spawn(|| {
                let audio_events = events::AudioEventPump::new(mem, &self.devices, audio_event_rx);
                audio_events.run();
            });

            self.cpus.run(self.vm, mem, &self.devices)
        })
    }
}

impl Devices {
    pub fn handle_mmio(
        &self,
        phys_addr: u64,
        is_write: bool,
        size: usize,
        value: u64,
        mem: &GuestMemory,
    ) -> std::result::Result<Option<u64>, ()> {
        let Some(route) = classify(phys_addr) else {
            return Err(());
        };

        let result = match route.device {
            MmioDevice::Uart => {
                if is_write {
                    self.uart.lock().unwrap().write(route.offset, value as u32);
                    None
                } else {
                    Some(self.uart.lock().unwrap().read(route.offset) as u64)
                }
            }
            MmioDevice::VirtioBlk => handle_virtio_mmio(&self.blk, route.offset, size, is_write, value, mem),
            MmioDevice::VirtioNet => handle_virtio_mmio(&self.net, route.offset, size, is_write, value, mem),
            MmioDevice::VirtioGpu => self.gpu.handle_mmio(route.offset, size, is_write, value),
            MmioDevice::VirtioInputKeyboard => {
                handle_virtio_mmio(&self.keyboard, route.offset, size, is_write, value, mem)
            }
            MmioDevice::VirtioInputTablet => handle_virtio_mmio(&self.tablet, route.offset, size, is_write, value, mem),
            MmioDevice::VirtioInputMouse => handle_virtio_mmio(&self.mouse, route.offset, size, is_write, value, mem),
            MmioDevice::VirtioSnd => handle_virtio_mmio(&self.snd, route.offset, size, is_write, value, mem),
            MmioDevice::VirtioConsole => handle_virtio_mmio(&self.console, route.offset, size, is_write, value, mem),
        };

        Ok(result)
    }
}

fn classify(phys_addr: u64) -> Option<MmioRoute> {
    MMIO_REGIONS.iter().find_map(|region| {
        (region.base..region.base + region.size)
            .contains(&phys_addr)
            .then(|| MmioRoute {
                device: region.device,
                offset: phys_addr - region.base,
            })
    })
}

fn handle_virtio_mmio<D: virtio::Device>(
    dev: &Mutex<virtio::MmioTransport<D>>,
    offset: u64,
    size: usize,
    is_write: bool,
    value: u64,
    mem: &GuestMemory,
) -> Option<u64> {
    let mut dev = dev.lock().unwrap();
    if is_write {
        dev.write(offset, size, value, mem);
        None
    } else {
        Some(dev.read(offset, size))
    }
}
