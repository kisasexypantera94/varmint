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
mod memory;
mod net;
mod stdio;
mod uart;
mod virtio;
mod vmm;

fn main() -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        panic!("do not run Varmint with sudo; the app requests vmnet privileges separately");
    }

    let Some(config) = config::locate() else {
        return Ok(());
    };

    vmm::run(&config)
}
