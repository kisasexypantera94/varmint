use applevisor::prelude::*;

mod app;
mod audio;
mod clipboard;
mod config;
mod cpu;
mod devices;
mod display;
mod irq;
mod launcher;
mod machine;
mod macos_ui;
mod memory;
mod net;
mod stdio;
mod storage;
mod uart;
mod virtio;
mod vmm;

fn main() -> Result<()> {
    match config::locate() {
        Some(config) => vmm::run(&config),
        None => {
            launcher::run();
            Ok(())
        }
    }
}
