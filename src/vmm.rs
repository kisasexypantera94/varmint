use crate::{
    app,
    devices::{Runtime, RuntimeEvent},
    display::{DisplayBuffer, DisplayEvent},
    machine::*,
    memory,
    runtime::RuntimeConfig,
    virtio,
};
use applevisor::prelude::*;
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::{
        Mutex,
        mpsc::{Receiver, Sender},
    },
    thread,
};
use winit::event_loop::EventLoopProxy;

fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
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
    display_proxy: EventLoopProxy<DisplayEvent>,
    config: &RuntimeConfig,
    disk: virtio::Blk,
    gicd_size: u64,
    gicr_size: u64,
) -> Result<()> {
    let image = read_file(&config.kernel)
        .unwrap_or_else(|error| panic!("failed to read kernel {}: {error}", config.kernel.display()));
    let initrd = config.initrd.as_ref().map(|path| {
        read_file(path).unwrap_or_else(|error| panic!("failed to read initrd {}: {error}", path.display()))
    });
    let initrd_range = initrd.as_ref().map(|data| {
        let end = checked_end(INITRD_START, data.len(), "initrd");
        (INITRD_START, end)
    });
    let dtb = build_fdt(
        config.memory_size as u64,
        config.vcpus,
        &config.kernel_args,
        initrd_range,
        gicd_size,
        gicr_size,
    )
    .unwrap_or_else(|error| panic!("failed to build guest FDT: {error}"));

    validate_boot_layout(config.memory_size, image.len(), initrd.as_deref(), dtb.len());

    let mut mem = memory::GuestMemory::new(vm.memory_create(config.memory_size)?);
    mem.map(RAM_START, MemPerms::RWX)?;
    mem.write(IMAGE_START, &image)?;
    if let Some(initrd) = &initrd {
        mem.write(INITRD_START, initrd)?;
    }
    mem.write(DTB_START, &dtb)?;

    Runtime::new(vm, runtime_event_tx, disk, config.vcpus)?.run(&mem, display, runtime_event_rx, &display_proxy)?;

    Ok(())
}

fn checked_end(start: u64, len: usize, name: &str) -> u64 {
    start
        .checked_add(len as u64)
        .unwrap_or_else(|| panic!("{name} address range overflows"))
}

fn validate_boot_layout(memory_size: usize, image_size: usize, initrd: Option<&[u8]>, dtb_size: usize) {
    let ram_end = checked_end(RAM_START, memory_size, "guest RAM");
    let image_end = checked_end(IMAGE_START, image_size, "kernel");
    if image_end > INITRD_START {
        panic!("kernel ends at 0x{image_end:x}, overlapping initrd region at 0x{INITRD_START:x}");
    }

    if let Some(initrd) = initrd {
        let initrd_end = checked_end(INITRD_START, initrd.len(), "initrd");
        if initrd_end > DTB_START {
            panic!("initrd ends at 0x{initrd_end:x}, overlapping FDT at 0x{DTB_START:x}");
        }
    }

    let dtb_end = checked_end(DTB_START, dtb_size, "FDT");
    if dtb_end > ram_end {
        panic!("FDT ends at 0x{dtb_end:x}, past guest RAM end at 0x{ram_end:x}");
    }
}

pub fn run() -> Result<()> {
    let config = RuntimeConfig::resolve();
    let disk = virtio::Blk::new(&config.disk, config.disk_size)
        .unwrap_or_else(|error| panic!("failed to open VM disk {}: {error}", config.disk.display()));

    let gicd_size = GicConfig::get_distributor_size()? as u64;
    let gicr_region_size = GicConfig::get_redistributor_region_size()? as u64;
    let gicr_stride = GicConfig::get_redistributor_size()? as u64;
    assert!(gicr_stride != 0);

    let max_vcpus = gicr_region_size / gicr_stride;
    if config.vcpus as u64 > max_vcpus {
        panic!(
            "vcpus={} exceeds the Hypervisor.framework GIC limit of {max_vcpus}",
            config.vcpus
        );
    }
    assert!(GICR_START > GICD_START + gicd_size);
    let gicr_size = gicr_stride * config.vcpus as u64;
    assert!(UART_START >= GICR_START + gicr_size);

    let mut gic_config = GicConfig::new();
    gic_config.set_distributor_base(GICD_START)?;
    gic_config.set_redistributor_base(GICR_START)?;

    let mut vm_cfg = VirtualMachineConfig::new();
    vm_cfg.set_ipa_granule(IpaGranule::HV_IPA_GRANULE_4KB)?;

    let vm = VirtualMachine::with_gic(vm_cfg, gic_config)?;

    let display = Mutex::new(DisplayBuffer::new());

    let (runtime_event_tx, runtime_event_rx) = std::sync::mpsc::channel();

    let event_loop = app::event_loop();
    let display_proxy = event_loop.create_proxy();

    thread::scope(|scope| {
        let vmm_runtime_event_tx = runtime_event_tx.clone();
        let vm = &vm;
        let display = &display;
        let config = &config;
        scope.spawn(move || {
            vmm_thread(
                vm,
                display,
                vmm_runtime_event_tx,
                runtime_event_rx,
                display_proxy,
                config,
                disk,
                gicd_size,
                gicr_size,
            )
        });

        app::run(event_loop, display, runtime_event_tx);
    });

    Ok(())
}
