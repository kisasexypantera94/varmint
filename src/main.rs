use applevisor::prelude::*;

mod angle_egl;
mod app;
mod audio;
mod clipboard;
mod cpu;
mod devices;
mod host_events;
mod iosurface;
mod irq;
mod machine;
mod memory;
mod net;
mod present;
mod uart;
mod virtio;
mod vmm;

fn main() -> Result<()> {
    vmm::run()
}
