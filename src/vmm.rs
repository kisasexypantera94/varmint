use crate::{
    app, audio, clipboard,
    config::VmConfig,
    devices::{HostBackends, Runtime, RuntimeEvent},
    display::{DisplayBuffer, DisplayEvent},
    machine::*,
    memory, net, stdio, virtio,
};
use applevisor::prelude::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
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

struct BootPayload {
    kernel: Vec<u8>,
    initrd: Option<Vec<u8>>,
}

impl BootPayload {
    fn load(config: &VmConfig) -> Self {
        let kernel = read_file(&config.kernel)
            .unwrap_or_else(|error| panic!("failed to read kernel {}: {error}", config.kernel.display()));
        let initrd = config.initrd.as_ref().map(|path| {
            read_file(path).unwrap_or_else(|error| panic!("failed to read initrd {}: {error}", path.display()))
        });
        Self { kernel, initrd }
    }
}

fn prepare_disk(config: &VmConfig) -> io::Result<()> {
    if config.disk.exists() {
        return Ok(());
    }

    let parent = config.disk.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = config.disk.file_name().and_then(|name| name.to_str()).unwrap_or("disk");
    let temporary = parent.join(format!(".{name}.partial.{}", std::process::id()));
    let _ = fs::remove_file(&temporary);

    let result = (|| {
        let source = File::open(&config.base_image)?;
        let output = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        let mut output = BufWriter::new(output);
        zstd::stream::copy_decode(BufReader::new(source), &mut output)?;
        output.flush()?;
        let output = output.into_inner().map_err(|error| error.into_error())?;
        output.sync_all()?;

        match fs::hard_link(&temporary, &config.disk) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    })();

    let _ = fs::remove_file(&temporary);
    if matches!(result, Ok(true)) {
        eprintln!(
            "created VM disk {} from {}",
            config.disk.display(),
            config.base_image.display()
        );
    }

    result.map(|_| ())
}

fn vmm_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    display: &Mutex<DisplayBuffer>,
    runtime_event_tx: Sender<RuntimeEvent>,
    runtime_event_rx: Receiver<RuntimeEvent>,
    audio_event_rx: Receiver<audio::BackendEvent>,
    display_proxy: EventLoopProxy<DisplayEvent>,
    config: &VmConfig,
    boot: BootPayload,
    backends: HostBackends,
    gicd_size: u64,
    gicr_size: u64,
) -> Result<()> {
    let initrd_range = boot.initrd.as_ref().map(|data| {
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

    validate_boot_layout(config.memory_size, boot.kernel.len(), boot.initrd.as_deref(), dtb.len());

    let mut mem = memory::GuestMemory::new(vm.memory_create(config.memory_size)?);
    mem.map(RAM_START, MemPerms::RWX)?;
    mem.write(IMAGE_START, &boot.kernel)?;
    if let Some(initrd) = &boot.initrd {
        mem.write(INITRD_START, initrd)?;
    }
    mem.write(DTB_START, &dtb)?;

    Runtime::new(vm, runtime_event_tx, backends, config.vcpus)?.run(
        &mem,
        display,
        runtime_event_rx,
        audio_event_rx,
        &display_proxy,
    )?;

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

pub fn run(config_path: &Path) -> Result<()> {
    let config = VmConfig::load(config_path);
    prepare_disk(&config).unwrap_or_else(|error| {
        panic!(
            "failed to create VM disk {} from {}: {error}",
            config.disk.display(),
            config.base_image.display()
        )
    });
    let disk = virtio::Blk::new(&config.disk, config.disk_size)
        .unwrap_or_else(|error| panic!("failed to open VM disk {}: {error}", config.disk.display()));
    let boot = BootPayload::load(&config);

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

    let net_rx_tx = runtime_event_tx.clone();
    let network = net::start(move |frame| {
        let _ = net_rx_tx.send(RuntimeEvent::NetRx(frame));
    })
    .unwrap_or_else(|error| panic!("failed to start vmnet networking: {error}"));

    let (audio_event_tx, audio_event_rx) = std::sync::mpsc::channel();
    let (_audio_backend, period_sink) = audio::Backend::new(move |event| {
        let _ = audio_event_tx.send(event);
    })
    .unwrap_or_else(|error| panic!("failed to start audio output: {error}"));

    let clipboard_change_tx = runtime_event_tx.clone();
    let clipboard = clipboard::start(move |payload| {
        let _ = clipboard_change_tx.send(RuntimeEvent::Clipboard(payload));
    });

    let serial_rx_tx = runtime_event_tx.clone();
    let serial = stdio::start(move |byte| {
        let _ = serial_rx_tx.send(RuntimeEvent::UartRx(byte));
    });

    let backends = HostBackends {
        disk,
        net: network,
        audio: period_sink,
        clipboard,
        serial,
    };

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
                audio_event_rx,
                display_proxy,
                config,
                boot,
                backends,
                gicd_size,
                gicr_size,
            )
        });

        app::run(event_loop, display, runtime_event_tx);
        std::process::exit(0);
    })
}
