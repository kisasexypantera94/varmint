use applevisor::prelude::*;

mod app;
mod audio;
mod clipboard;
mod config;
mod cpu;
mod devices;
mod display;
mod irq;
mod machine;
mod macos_ui;
mod memory;
mod net;
mod stdio;
mod uart;
mod virtio;
mod vmm;

fn main() -> Result<()> {
    let Some(config) = config::locate() else {
        return Ok(());
    };

    vmm::run(&config)
}
