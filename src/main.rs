use applevisor::prelude::*;

mod app;
mod audio;
mod clipboard;
mod cpu;
mod devices;
mod display;
mod irq;
mod machine;
mod memory;
mod net;
mod uart;
mod virtio;
mod vmm;

fn main() -> Result<()> {
    vmm::run()
}
