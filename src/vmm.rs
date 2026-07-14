use crate::{
    app, audio, clipboard,
    cpu::CpuRuntime,
    devices::{DeviceThreadRequest, VmDevices, gpu_owner_thread},
    host_events::{HostEvent, HostEventPump},
    irq,
    machine::*,
    memory, net, uart, virtio,
};
use applevisor::prelude::*;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
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

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> std::io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn stdin_thread(uart: &Mutex<uart::Uart>) {
    let _raw = RawModeGuard::new().unwrap();
    let stdin = std::io::stdin();
    let mut buf = [0u8; 1];

    const PREFIX: u8 = 0x1d; // Ctrl-]
    let mut got_prefix = false;

    eprintln!("[VM] Press Ctrl-] x to exit");

    loop {
        match stdin.lock().read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                let b = buf[0];

                if got_prefix {
                    got_prefix = false;
                    match b {
                        b'x' => {
                            eprintln!("Received break command");
                            break;
                        }
                        _ => eprint!("unknown command: {b:#x}\r\n"),
                    }
                    continue;
                }

                if b == PREFIX {
                    got_prefix = true;
                    continue;
                }

                uart.lock().unwrap().enqueue(b);
            }
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        }
    }
}

fn vmm_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    display: &Mutex<virtio::gpu::DisplayBuffer>,
    host_tx: Sender<HostEvent>,
    host_rx: Receiver<HostEvent>,
) -> Result<()> {
    let image = read_file("/Users/dvgr/varmint-kernels/debian-4k/vmlinuz-6.12.90+deb13.1-arm64").unwrap();
    let initrd = read_file("/Users/dvgr/varmint-kernels/debian-4k/initrd.img-6.12.90+deb13.1-arm64").unwrap();
    let dtb = read_file("./artifacts/guest.dtb").unwrap();

    let mut mem = memory::GuestMemory::new(vm.memory_create(RAM_SIZE)?);
    mem.map(RAM_START, MemPerms::RWX)?;
    mem.write(IMAGE_START, &image)?;
    mem.write(INITRD_START, &initrd)?;
    mem.write(DTB_START, &dtb)?;

    let (spi_int_start, _) = GicConfig::get_spi_interrupt_range()?;

    let uart = Mutex::new(uart::Uart::new(irq::IrqLine::new(vm, spi_int_start + UART_SPI_OFFSET)));

    let (clipboard_out_tx, clipboard_out_rx) = std::sync::mpsc::channel::<Vec<u8>>();

    let virtio_blk_dev = virtio::Blk::new("dev0.img", 40 * 1024 * 1024 * 1024);
    let virtio_blk = Mutex::new(virtio::MmioTransport::new(
        virtio_blk_dev,
        irq::IrqLine::new(vm, spi_int_start + VIRTBLK_SPI_OFFSET),
    ));

    let net_ready_tx = host_tx.clone();
    let iface = Mutex::new(net::vmnet::Backend::new().unwrap());
    iface
        .lock()
        .unwrap()
        .set_event_callback(move || {
            let _ = net_ready_tx.send(HostEvent::NetReady);
        })
        .unwrap();
    let net_tx = host_tx.clone();
    let virtio_net_dev = virtio::Net::new(iface.lock().unwrap().mac(), move |frame| {
        let _ = net_tx.send(HostEvent::NetTx(frame));
    });
    let virtio_net = Mutex::new(virtio::MmioTransport::new(
        virtio_net_dev,
        irq::IrqLine::new(vm, spi_int_start + VIRTNET_SPI_OFFSET),
    ));

    let virtio_gpu_irq = irq::IrqLine::new(vm, spi_int_start + VIRTGPU_SPI_OFFSET);

    let virtio_input_keyboard_dev = virtio::Input::keyboard();
    let virtio_input_keyboard = Mutex::new(virtio::MmioTransport::new(
        virtio_input_keyboard_dev,
        irq::IrqLine::new(vm, spi_int_start + VIRTINPUT_KEYBOARD_SPI_OFFSET),
    ));

    let virtio_input_tablet_dev = virtio::Input::tablet();
    let virtio_input_tablet = Mutex::new(virtio::MmioTransport::new(
        virtio_input_tablet_dev,
        irq::IrqLine::new(vm, spi_int_start + VIRTINPUT_TABLET_SPI_OFFSET),
    ));

    let virtio_input_mouse_dev = virtio::Input::mouse();
    let virtio_input_mouse = Mutex::new(virtio::MmioTransport::new(
        virtio_input_mouse_dev,
        irq::IrqLine::new(vm, spi_int_start + VIRTINPUT_MOUSE_SPI_OFFSET),
    ));

    let audio_tx = host_tx.clone();
    let (_audio_backend, period_sink) = audio::coreaudio::Backend::new(move |event| {
        let _ = audio_tx.send(HostEvent::Audio(event));
    })
    .unwrap();
    let virtio_snd_dev = virtio::Snd::new(period_sink);
    let virtio_snd = Mutex::new(virtio::MmioTransport::new(
        virtio_snd_dev,
        irq::IrqLine::new(vm, spi_int_start + VIRTSND_SPI_OFFSET),
    ));

    let virtio_console_dev = virtio::Console::new(clipboard_out_tx);
    let virtio_console = Mutex::new(virtio::MmioTransport::new(
        virtio_console_dev,
        irq::IrqLine::new(vm, spi_int_start + VIRTCONSOLE_SPI_OFFSET),
    ));

    let mem_ref = &mem;
    let (gpu_tx, gpu_rx) = std::sync::mpsc::channel::<DeviceThreadRequest>();

    thread::scope(|s| -> Result<()> {
        let cpus = CpuRuntime::new(vm, IMAGE_START, DTB_START)?;
        s.spawn(|| stdin_thread(&uart));

        let gpu_mem = mem_ref;
        let gpu_display = display;
        let gpu_owner_tx = gpu_tx.clone();
        s.spawn(move || {
            gpu_owner_thread(gpu_mem, gpu_display, gpu_rx, virtio_gpu_irq, gpu_owner_tx);
        });

        let clipboard_tx = host_tx.clone();
        s.spawn(move || {
            clipboard::run(clipboard_out_rx, move |payload| {
                let _ = clipboard_tx.send(HostEvent::Clipboard(payload));
            });
        });

        let devices = VmDevices {
            uart: &uart,
            blk: &virtio_blk,
            net: &virtio_net,
            gpu_tx: &gpu_tx,
            keyboard: &virtio_input_keyboard,
            tablet: &virtio_input_tablet,
            mouse: &virtio_input_mouse,
            snd: &virtio_snd,
            console: &virtio_console,
        };

        let host_devices = devices;

        s.spawn(move || {
            let mut host_events = HostEventPump::new(mem_ref, host_devices, &iface, host_rx);

            host_events.run();
        });

        cpus.run(vm, mem_ref, devices)?;

        Ok(())
    })?;

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

    let display = Mutex::new(virtio::gpu::DisplayBuffer::new());

    let (host_tx, host_rx) = std::sync::mpsc::channel();

    thread::scope(|s| {
        let vmm_host_tx = host_tx.clone();
        let vm_ref = &vm;
        let display_ref = &display;
        s.spawn(move || vmm_thread(vm_ref, display_ref, vmm_host_tx, host_rx));

        app::run(&display, host_tx);
    });

    Ok(())
}
