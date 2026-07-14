use crate::{
    app,
    devices::{Runtime, RuntimeEvent},
    display::DisplayBuffer,
    machine::*,
    memory,
};
use applevisor::prelude::*;
use std::{
    fs::File,
    io::Read,
    sync::{
        Mutex,
        mpsc::{Receiver, Sender},
    },
    thread,
};

fn read_file(path: &str) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

fn vmm_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    display: &Mutex<DisplayBuffer>,
    runtime_event_tx: Sender<RuntimeEvent>,
    runtime_event_rx: Receiver<RuntimeEvent>,
) -> Result<()> {
    let image = read_file("/Users/dvgr/varmint-kernels/debian-4k/vmlinuz-6.12.90+deb13.1-arm64").unwrap();
    let initrd = read_file("/Users/dvgr/varmint-kernels/debian-4k/initrd.img-6.12.90+deb13.1-arm64").unwrap();
    let dtb = read_file("./artifacts/guest.dtb").unwrap();

    let mut mem = memory::GuestMemory::new(vm.memory_create(RAM_SIZE)?);
    mem.map(RAM_START, MemPerms::RWX)?;
    mem.write(IMAGE_START, &image)?;
    mem.write(INITRD_START, &initrd)?;
    mem.write(DTB_START, &dtb)?;

    Runtime::new(vm, runtime_event_tx)?.run(&mem, display, runtime_event_rx)?;

    Ok(())
}

pub fn run() -> Result<()> {
    let gicd_size = GicConfig::get_distributor_size()?;
    assert!(GICR_START > GICD_START + gicd_size as u64);

    let mut gic_config = GicConfig::new();
    gic_config.set_distributor_base(GICD_START)?;
    gic_config.set_redistributor_base(GICR_START)?;

    let mut vm_cfg = VirtualMachineConfig::new();
    vm_cfg.set_ipa_granule(IpaGranule::HV_IPA_GRANULE_4KB)?;

    let vm = VirtualMachine::with_gic(vm_cfg, gic_config)?;
    let display = Mutex::new(DisplayBuffer::new());
    let (runtime_event_tx, runtime_event_rx) = std::sync::mpsc::channel();

    thread::scope(|scope| {
        let vmm_runtime_event_tx = runtime_event_tx.clone();
        scope.spawn(|| vmm_thread(&vm, &display, vmm_runtime_event_tx, runtime_event_rx));

        app::run(&display, runtime_event_tx);
    });

    Ok(())
}
