use crate::{
    display::DisplayBuffer,
    irq::IrqLine,
    memory::GuestMemory,
    virtio::{
        self,
        virgl_ffi::{VirglFence, VirglRenderer},
    },
};
use std::{
    sync::{
        Mutex,
        mpsc::{Receiver, Sender, SyncSender},
    },
    thread,
};

enum Request {
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
    Event(virtio::gpu::ExternalEvent),
}

pub struct Handle {
    tx: Sender<Request>,
}

pub struct Worker {
    rx: Receiver<Request>,
    irq: IrqLine,
    tx: Sender<Request>,
}

pub fn channel(irq: IrqLine) -> (Handle, Worker) {
    let (tx, rx) = std::sync::mpsc::channel();
    (Handle { tx: tx.clone() }, Worker { rx, irq, tx })
}

impl Handle {
    pub fn handle_mmio(&self, offset: u64, size: usize, is_write: bool, value: u64) -> Option<u64> {
        if is_write && offset == virtio::QUEUE_NOTIFY_OFFSET {
            self.tx
                .send(Request::MmioWriteAsync { offset, size, value })
                .expect("gpu owner thread exited");
            return None;
        }

        let (resp_tx, resp_rx) = std::sync::mpsc::sync_channel(0);
        self.tx
            .send(Request::Mmio {
                is_write,
                offset,
                size,
                value,
                resp: resp_tx,
            })
            .expect("gpu owner thread exited");
        resp_rx.recv().expect("gpu owner thread dropped MMIO response")
    }

    pub fn send_event(&self, event: virtio::gpu::ExternalEvent) {
        let _ = self.tx.send(Request::Event(event));
    }
}

impl Worker {
    pub fn run(self, mem: &GuestMemory, display: &Mutex<DisplayBuffer>) {
        let Self { rx, irq, tx } = self;
        let mut renderer = build_virglrenderer(tx).expect("virglrenderer init failed");
        let gpu_dev = virtio::Gpu::new(display, &mut renderer);
        let mut gpu = virtio::MmioTransport::new(gpu_dev, irq);

        while let Ok(req) = rx.recv() {
            match req {
                Request::Mmio {
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
                Request::MmioWriteAsync { offset, size, value } => {
                    gpu.write(offset, size, value, mem);
                }
                Request::Event(event) => {
                    gpu.handle_external_event(event, mem);
                }
            }
        }
    }
}

fn build_virglrenderer(gpu_tx: Sender<Request>) -> virtio::virgl_ffi::VirglResult<VirglRenderer> {
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
                .send(Request::Event(virtio::gpu::ExternalEvent::PollRendererFences))
                .is_err()
            {
                break;
            }
        }
    });

    let renderer = VirglRenderer::new(move |fence: VirglFence| {
        let _ = gpu_tx.send(Request::Event(virtio::gpu::ExternalEvent::FenceSignaled {
            ctx_id: fence.ctx_id,
            ring_idx: fence.ring_idx.map(|r| r as u8),
            fence_id: fence.fence_id,
        }));
    })?;

    Ok(renderer)
}
