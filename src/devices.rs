use crate::{
    irq,
    machine::{DeviceOwner, MmioDevice, MmioRoute, VIRTIO_MMIO_QUEUE_NOTIFY},
    memory, uart,
    virtio::{
        self,
        virgl_ffi::{VirglFence, VirglRenderer},
    },
};
use std::{
    io::{self, Write},
    sync::{
        Mutex,
        mpsc::{Receiver, Sender, SyncSender},
    },
    thread,
};

pub enum DeviceThreadRequest {
    Mmio {
        is_write: bool,
        offset: u64,
        size: usize,
        value: u64,
        resp: SyncSender<Option<u64>>,
    },
    MmioWriteAsync {
        offset: u64,
        size: usize,
        value: u64,
    },
    GpuEvent(virtio::gpu::ExternalEvent),
}

#[derive(Clone, Copy)]
pub struct VmDevices<'a> {
    pub uart: &'a Mutex<uart::Uart>,
    pub blk: &'a Mutex<virtio::MmioTransport<virtio::Blk>>,
    pub net: &'a Mutex<virtio::MmioTransport<virtio::Net>>,
    pub gpu_tx: &'a Sender<DeviceThreadRequest>,
    pub keyboard: &'a Mutex<virtio::MmioTransport<virtio::Input>>,
    pub tablet: &'a Mutex<virtio::MmioTransport<virtio::Input>>,
    pub mouse: &'a Mutex<virtio::MmioTransport<virtio::Input>>,
    pub snd: &'a Mutex<virtio::MmioTransport<virtio::Snd>>,
    pub console: &'a Mutex<virtio::MmioTransport<virtio::Console>>,
}

fn send_gpu_mmio(
    gpu_tx: &Sender<DeviceThreadRequest>,
    offset: u64,
    size: usize,
    is_write: bool,
    value: u64,
) -> Option<u64> {
    if is_write && offset == VIRTIO_MMIO_QUEUE_NOTIFY {
        gpu_tx
            .send(DeviceThreadRequest::MmioWriteAsync { offset, size, value })
            .expect("gpu owner thread exited");
        return None;
    }

    let (resp_tx, resp_rx) = std::sync::mpsc::sync_channel(0);
    gpu_tx
        .send(DeviceThreadRequest::Mmio {
            is_write,
            offset,
            size,
            value,
            resp: resp_tx,
        })
        .expect("gpu owner thread exited");
    resp_rx.recv().expect("gpu owner thread dropped MMIO response")
}

pub fn send_gpu_event(gpu_tx: &Sender<DeviceThreadRequest>, event: virtio::gpu::ExternalEvent) {
    let _ = gpu_tx.send(DeviceThreadRequest::GpuEvent(event));
}

pub fn gpu_owner_thread(
    mem: &memory::GuestMemory,
    display: &Mutex<virtio::gpu::DisplayBuffer>,
    rx: Receiver<DeviceThreadRequest>,
    irq: irq::IrqLine,
    gpu_tx: Sender<DeviceThreadRequest>,
) {
    let mut renderer = build_virglrenderer(gpu_tx).expect("virglrenderer init failed");
    let gpu_dev = virtio::Gpu::new(display, &mut renderer);
    let mut gpu = virtio::MmioTransport::new(gpu_dev, irq);

    while let Ok(req) = rx.recv() {
        match req {
            DeviceThreadRequest::Mmio {
                is_write,
                offset,
                size,
                value,
                resp,
            } => {
                let ret = if is_write {
                    gpu.write(offset, size, value, mem);
                    None
                } else {
                    Some(gpu.read(offset, size))
                };
                let _ = resp.send(ret);
            }
            DeviceThreadRequest::MmioWriteAsync { offset, size, value } => {
                gpu.write(offset, size, value, mem);
            }
            DeviceThreadRequest::GpuEvent(event) => {
                gpu.handle_external_event(event, mem);
            }
        }
    }
}

fn handle_virtio_mmio<D: virtio::Device>(
    dev: &Mutex<virtio::MmioTransport<D>>,
    offset: u64,
    size: usize,
    is_write: bool,
    value: u64,
    mem: &memory::GuestMemory,
) -> Option<u64> {
    let mut dev = dev.lock().unwrap();
    if is_write {
        dev.write(offset, size, value, mem);
        None
    } else {
        Some(dev.read(offset, size))
    }
}

fn handle_inline_mmio(
    route: MmioRoute,
    is_write: bool,
    size: usize,
    value: u64,
    mem: &memory::GuestMemory,
    devices: &VmDevices<'_>,
) -> Option<u64> {
    match route.device {
        MmioDevice::Uart => {
            if is_write {
                devices.uart.lock().unwrap().write(route.offset, value as u32, |value| {
                    io::stdout().write_all(&[value as u8]).unwrap();
                    io::stdout().flush().unwrap();
                });
                None
            } else {
                Some(devices.uart.lock().unwrap().read(route.offset) as u64)
            }
        }
        MmioDevice::VirtioBlk => handle_virtio_mmio(devices.blk, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioNet => handle_virtio_mmio(devices.net, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioGpu => panic!("virtio-gpu is not an inline device"),
        MmioDevice::VirtioInputKeyboard => {
            handle_virtio_mmio(devices.keyboard, route.offset, size, is_write, value, mem)
        }
        MmioDevice::VirtioInputTablet => handle_virtio_mmio(devices.tablet, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioInputMouse => handle_virtio_mmio(devices.mouse, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioSnd => handle_virtio_mmio(devices.snd, route.offset, size, is_write, value, mem),
        MmioDevice::VirtioConsole => handle_virtio_mmio(devices.console, route.offset, size, is_write, value, mem),
    }
}

pub fn handle_routed_mmio(
    route: MmioRoute,
    is_write: bool,
    size: usize,
    value: u64,
    mem: &memory::GuestMemory,
    devices: &VmDevices<'_>,
) -> Option<u64> {
    match route.owner {
        DeviceOwner::Inline => handle_inline_mmio(route, is_write, size, value, mem, devices),
        DeviceOwner::Gpu => send_gpu_mmio(devices.gpu_tx, route.offset, size, is_write, value),
    }
}

fn build_virglrenderer(gpu_tx: Sender<DeviceThreadRequest>) -> virtio::virgl_ffi::VirglResult<VirglRenderer> {
    let poll_tx = gpu_tx.clone();

    let fence_poll_interval = std::env::var("VARMINT_FENCE_POLL_US")
        .unwrap_or("1000".into())
        .parse::<u64>()
        .ok()
        .filter(|&v| v > 0)
        .map(std::time::Duration::from_micros)
        .expect("invalid fence poll duration");

    thread::spawn(move || {
        loop {
            std::thread::sleep(fence_poll_interval);

            if poll_tx
                .send(DeviceThreadRequest::GpuEvent(
                    virtio::gpu::ExternalEvent::PollRendererFences,
                ))
                .is_err()
            {
                break;
            }
        }
    });

    let renderer = VirglRenderer::new(move |fence: VirglFence| {
        let _ = gpu_tx.send(DeviceThreadRequest::GpuEvent(
            virtio::gpu::ExternalEvent::FenceSignaled {
                ctx_id: fence.ctx_id,
                ring_idx: fence.ring_idx.map(|r| r as u8),
                fence_id: fence.fence_id,
            },
        ));
    })?;

    Ok(renderer)
}
