use applevisor::prelude::*;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use std::{
    fs::File,
    io::{self, Read, Write},
    sync::{
        Mutex,
        mpsc::{Receiver, Sender},
    },
    thread,
};

mod helpers;
mod irq;
mod linux;
mod net;
mod sys_reg;
mod uart;
mod virtio;

const RAM_START: u64 = 0x40000000;
const RAM_SIZE: usize = 0x20000000;

const KERNEL_TEXT_OFFSET: u64 = 0x0;
const IMAGE_START: u64 = RAM_START + KERNEL_TEXT_OFFSET;
const INITRD_START: u64 = 0x48000000;
const DTB_START: u64 = 0x4F000000;
const GICD_START: u64 = 0x08000000;
const GICR_START: u64 = 0x080A0000;
const PSTATE_EL1H_DAIF_MASKED: u64 = 0x3c5;

const UART_START: u64 = 0x09000000;
const UART_SIZE: u64 = 0x1000;
const UART_SPI_OFFSET: u32 = 1;

const VIRTBLK_START: u64 = 0x0a000000;
const VIRTBLK_SIZE: u64 = 0x1000;
const VIRTBLK_SPI_OFFSET: u32 = 32;

const VIRTNET_START: u64 = 0x0a001000;
const VIRTNET_SIZE: u64 = 0x1000;
const VIRTNET_SPI_OFFSET: u32 = 33;

const VIRTGPU_START: u64 = 0x0a002000;
const VIRTGPU_SIZE: u64 = 0x1000;
const VIRTGPU_SPI_OFFSET: u32 = 34;

const VIRTINPUT_START: u64 = 0x0a003000;
const VIRTINPUT_SIZE: u64 = 0x1000;
const VIRTINPUT_SPI_OFFSET: u32 = 35;

enum MmioRegion {
    Uart(u64),
    VirtioBlk(u64),
    VirtioNet(u64),
    VirtioGpu(u64),
    VirtioInput(u64),
}

fn classify(phys_addr: u64) -> Option<MmioRegion> {
    const REGIONS: &[(u64, u64, fn(u64) -> MmioRegion)] = &[
        (UART_START, UART_SIZE, MmioRegion::Uart),
        (VIRTBLK_START, VIRTBLK_SIZE, MmioRegion::VirtioBlk),
        (VIRTNET_START, VIRTNET_SIZE, MmioRegion::VirtioNet),
        (VIRTGPU_START, VIRTGPU_SIZE, MmioRegion::VirtioGpu),
        (VIRTINPUT_START, VIRTINPUT_SIZE, MmioRegion::VirtioInput),
    ];
    REGIONS.iter().find_map(|&(base, size, ctor)| {
        (base..base + size)
            .contains(&phys_addr)
            .then(|| ctor(phys_addr - base))
    })
}

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

fn stdin_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    handle: VcpuHandle,
    uart: &Mutex<uart::Uart>,
) {
    let _raw = RawModeGuard::new().unwrap();
    let stdin = std::io::stdin();
    let mut buf = [0u8; 1];

    // Escape sequence: Ctrl-] then 'x' to exit, Ctrl-] then ']' to send literal Ctrl-]
    const PREFIX: u8 = 0x1d; // Ctrl-]
    let mut got_prefix = false;

    eprintln!("[VM] Press Ctrl-] x to exit");

    loop {
        match stdin.lock().read(&mut buf) {
            Ok(0) => break, // EOF
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

                if uart.lock().unwrap().enqueue(b) {
                    vm.vcpus_exit(&[handle.clone()]).unwrap();
                }
            }
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        }
    }
}

fn mmio_read_reg(vcpu: &Vcpu, rt: u64) -> Result<u32> {
    Ok(match helpers::reg_from_rt(rt) {
        Some(reg) => vcpu.get_reg(reg)? as u32,
        None => 0,
    })
}

fn mmio_write_reg(vcpu: &Vcpu, rt: u64, value: u64) -> Result<()> {
    if let Some(reg) = helpers::reg_from_rt(rt) {
        vcpu.set_reg(reg, value)?;
    }
    Ok(())
}

#[derive(Debug, Copy, Clone)]
struct HostKeyEvent {
    code: u16,
    pressed: bool,
}

fn window_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    handle: VcpuHandle,
    display: &Mutex<virtio::gpu::DisplayBuffer>,
    input_tx: Sender<HostKeyEvent>,
) {
    let gpu_width = 1024usize;
    let gpu_height = 768usize;

    let mut window = Window::new(
        "varmint virtio-gpu",
        gpu_width,
        gpu_height,
        WindowOptions::default(),
    )
    .unwrap();

    while window.is_open() {
        {
            let mut display = display.lock().unwrap();

            window
                .update_with_buffer(&display.pixels, display.width, display.height)
                .unwrap();

            display.dirty = false;
        }

        for key in window.get_keys_pressed(KeyRepeat::No) {
            if let Some(code) = helpers::minifb_to_linux_key(key) {
                let _ = input_tx.send(HostKeyEvent {
                    code,
                    pressed: true,
                });
                vm.vcpus_exit(&[handle.clone()]).unwrap();
            }
        }

        for key in window.get_keys_released() {
            if let Some(code) = helpers::minifb_to_linux_key(key) {
                let _ = input_tx.send(HostKeyEvent {
                    code,
                    pressed: false,
                });
                vm.vcpus_exit(&[handle.clone()]).unwrap();
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn run_loop(
    vm: &VirtualMachineInstance<GicEnabled>,
    vcpu: &Vcpu,
    mem: &mut applevisor::memory::Memory,
    uart: &Mutex<uart::Uart>,
    uart_irq: &mut irq::IrqLine,
    virtio_blk: &mut virtio::MmioTransport<virtio::Blk>,
    virtio_blk_irq: &mut irq::IrqLine,
    virtio_net: &mut virtio::MmioTransport<virtio::Net>,
    virtio_net_irq: &mut irq::IrqLine,
    iface: &mut net::VmnetBackend,
    virtio_gpu: &mut virtio::MmioTransport<virtio::Gpu>,
    virtio_gpu_irq: &mut irq::IrqLine,
    virtio_input: &mut virtio::MmioTransport<virtio::Input>,
    virtio_input_irq: &mut irq::IrqLine,
    input_rx: &Receiver<HostKeyEvent>,
) -> Result<()> {
    let mut net_buf = vec![0; iface.max_packet_size() as usize];

    loop {
        loop {
            let n_read = iface.read(&mut net_buf).unwrap();
            if n_read > 0 {
                virtio_net.deliver_external(&net_buf[..n_read], mem);
            } else {
                break;
            }
        }

        while let Ok(event) = input_rx.try_recv() {
            virtio_input.push_key(event.code, event.pressed, mem);
        }

        uart_irq.sync(vm, uart.lock().unwrap().is_asserted())?;
        virtio_blk_irq.sync(vm, virtio_blk.is_asserted())?;
        virtio_net_irq.sync(vm, virtio_net.is_asserted())?;
        virtio_gpu_irq.sync(vm, virtio_gpu.is_asserted())?;
        virtio_input_irq.sync(vm, virtio_input.is_asserted())?;

        vcpu.run()?;
        let exit = vcpu.get_exit_info();

        match exit.reason {
            ExitReason::EXCEPTION => {
                // https://developer.arm.com/documentation/ddi0601/2026-03/AArch64-Registers/ESR-EL2--Exception-Syndrome-Register--EL2-
                let esr_el2_like = exit.exception.syndrome;
                let phys_addr = exit.exception.physical_address;
                let pc = vcpu.get_reg(Reg::PC)?;

                let ec = (esr_el2_like >> 26) & 0b111111;

                match ec {
                    0x18 => {
                        if sys_reg::handle_trap(vcpu, esr_el2_like, pc)? {
                            continue;
                        }

                        let (sysreg, rt, is_read) = sys_reg::decode_sysreg(esr_el2_like);
                        panic!(
                            "unhandled sysreg trap: {:?}, rt={}, {}, esr=0x{:x}, pc=0x{:x}",
                            sysreg,
                            rt,
                            if is_read { "read/MRS" } else { "write/MSR" },
                            esr_el2_like,
                            pc
                        );
                    }

                    0x24 | 0x25 => {
                        let is_write = ((esr_el2_like >> 6) & 1) == 1;
                        let rt = (esr_el2_like >> 16) & 0b11111;
                        let size = 1usize << ((esr_el2_like >> 22) & 0b11);

                        match classify(phys_addr) {
                            Some(MmioRegion::Uart(offset)) => {
                                if is_write {
                                    let value = mmio_read_reg(vcpu, rt)?;
                                    uart.lock().unwrap().write(offset, value, |value| {
                                        io::stdout().write_all(&[value as u8]).unwrap();
                                        io::stdout().flush().unwrap();
                                    });
                                } else {
                                    let value = uart.lock().unwrap().read(offset);
                                    mmio_write_reg(vcpu, rt, value as u64)?;
                                }
                            }
                            Some(MmioRegion::VirtioBlk(offset)) => {
                                if is_write {
                                    let value = mmio_read_reg(vcpu, rt)?;
                                    virtio_blk.write(offset, size, value as u64, mem);
                                } else {
                                    let value = virtio_blk.read(offset, size);
                                    mmio_write_reg(vcpu, rt, value)?;
                                }
                            }
                            Some(MmioRegion::VirtioNet(offset)) => {
                                if is_write {
                                    let value = mmio_read_reg(vcpu, rt)?;
                                    virtio_net.write(offset, size, value as u64, mem);
                                    while let Some(frame) = virtio_net.pop_external() {
                                        iface.write(&frame).unwrap();
                                    }
                                } else {
                                    let value = virtio_net.read(offset, size);
                                    mmio_write_reg(vcpu, rt, value)?;
                                }
                            }
                            Some(MmioRegion::VirtioGpu(offset)) => {
                                if is_write {
                                    let value = mmio_read_reg(vcpu, rt)?;
                                    virtio_gpu.write(offset, size, value as u64, mem);
                                } else {
                                    let value = virtio_gpu.read(offset, size);
                                    mmio_write_reg(vcpu, rt, value)?;
                                }
                            }
                            Some(MmioRegion::VirtioInput(offset)) => {
                                if is_write {
                                    let value = mmio_read_reg(vcpu, rt)?;
                                    virtio_input.write(offset, size, value as u64, mem);
                                } else {
                                    let value = virtio_input.read(offset, size);
                                    mmio_write_reg(vcpu, rt, value)?;
                                }
                            }
                            None => {
                                panic!(
                                    "unhandled data abort trap: ec={}, rt={}, {}, esr=0x{:x}, pc=0x{:x}, addr=0x{:x}",
                                    ec,
                                    rt,
                                    if is_write { "write" } else { "read" },
                                    esr_el2_like,
                                    pc,
                                    phys_addr,
                                );
                            }
                        }

                        vcpu.set_reg(Reg::PC, pc + 4)?;
                    }

                    0x20 | 0x21 => {
                        panic!(
                            "instruction abort: fault=0x{:x}, esr=0x{:x}, pc=0x{:x}",
                            exit.exception.physical_address, esr_el2_like, pc
                        );
                    }

                    _ => {
                        panic!(
                            "unexpected exception ec=0x{:x}, esr=0x{:x}, pc=0x{:x}",
                            ec, esr_el2_like, pc
                        );
                    }
                }
            }
            ExitReason::CANCELED => (),
            _ => eprintln!("unexpected exit reason: {:?}", exit),
        }
    }
}

fn vmm_thread(
    vm: &VirtualMachineInstance<GicEnabled>,
    handle_tx: Sender<VcpuHandle>,
    uart: &Mutex<uart::Uart>,
    display: &Mutex<virtio::gpu::DisplayBuffer>,
    input_rx: Receiver<HostKeyEvent>,
) -> Result<()> {
    let image = read_file("./artifacts/debian-arm64/installed-vmlinuz").unwrap();
    let initrd = read_file("./artifacts/debian-arm64/installed-initrd.gz").unwrap();
    let dtb = read_file("./artifacts/guest.dtb").unwrap();

    let image_header = linux::parse_image_header(&image).unwrap();
    eprintln!("Image header: {:?}", image_header);

    let vcpu = vm.vcpu_create()?;
    vcpu.set_sys_reg(SysReg::MPIDR_EL1, 0)?;

    handle_tx.send(vcpu.get_handle()).unwrap();

    let (spi_int_start, _) = GicConfig::get_spi_interrupt_range()?;

    let mut mem = vm.memory_create(RAM_SIZE)?;
    mem.map(RAM_START, MemPerms::RWX)?;
    mem.write(IMAGE_START, &image)?;
    mem.write(INITRD_START, &initrd)?;
    mem.write(DTB_START, &dtb)?;

    vcpu.set_reg(Reg::CPSR, PSTATE_EL1H_DAIF_MASKED)?; // Start in EL1
    vcpu.set_reg(Reg::PC, IMAGE_START)?;
    vcpu.set_reg(Reg::X0, DTB_START)?;
    vcpu.set_reg(Reg::X1, 0)?;
    vcpu.set_reg(Reg::X2, 0)?;
    vcpu.set_reg(Reg::X3, 0)?;

    let mut uart_irq = irq::IrqLine::new(spi_int_start + UART_SPI_OFFSET, false);

    let virtio_blk_dev = virtio::Blk::new("dev0.img", 8 * 1024 * 1024 * 1024);
    let mut virtio_blk = virtio::MmioTransport::new(virtio_blk_dev);
    let mut virtio_blk_irq = irq::IrqLine::new(spi_int_start + VIRTBLK_SPI_OFFSET, false);

    let mut iface = net::VmnetBackend::new().unwrap();
    let virtio_net_dev = virtio::Net::new(iface.mac());
    let mut virtio_net = virtio::MmioTransport::new(virtio_net_dev);
    let mut virtio_net_irq = irq::IrqLine::new(spi_int_start + VIRTNET_SPI_OFFSET, false);

    let virtio_gpu_dev = virtio::Gpu::new(display);
    let mut virtio_gpu = virtio::MmioTransport::new(virtio_gpu_dev);
    let mut virtio_gpu_irq = irq::IrqLine::new(spi_int_start + VIRTGPU_SPI_OFFSET, false);

    let virtio_input_dev = virtio::Input::new();
    let mut virtio_input = virtio::MmioTransport::new(virtio_input_dev);
    let mut virtio_input_irq = irq::IrqLine::new(spi_int_start + VIRTINPUT_SPI_OFFSET, false);

    thread::scope(|s| -> Result<()> {
        let vmnet_vcpu_handle = vcpu.get_handle();
        let (signal_tx, signal_rx) = std::sync::mpsc::sync_channel::<()>(1);

        s.spawn(move || {
            while signal_rx.recv().is_ok() {
                vm.vcpus_exit(&[vmnet_vcpu_handle.clone()]).unwrap();
            }
        });

        iface
            .set_event_callback(move || {
                let _ = signal_tx.try_send(());
            })
            .unwrap();

        run_loop(
            vm,
            &vcpu,
            &mut mem,
            uart,
            &mut uart_irq,
            &mut virtio_blk,
            &mut virtio_blk_irq,
            &mut virtio_net,
            &mut virtio_net_irq,
            &mut iface,
            &mut virtio_gpu,
            &mut virtio_gpu_irq,
            &mut virtio_input,
            &mut virtio_input_irq,
            &input_rx,
        )
    })?;

    Ok(())
}

fn main() -> Result<()> {
    let gicd_size = GicConfig::get_distributor_size()?;
    assert!(GICR_START > GICD_START + gicd_size as u64);

    let mut gic_config = GicConfig::new();
    gic_config.set_distributor_base(GICD_START)?;
    gic_config.set_redistributor_base(GICR_START)?;

    let vm = VirtualMachine::with_gic(VirtualMachineConfig::default(), gic_config)?;

    let uart = Mutex::new(uart::Uart::new());

    let (handle_tx, handle_rx) = std::sync::mpsc::channel();

    let display = Mutex::new(virtio::gpu::DisplayBuffer::new(1024, 768));

    let (input_tx, input_rx) = std::sync::mpsc::channel();

    thread::scope(|s| {
        let vm_ref = &vm;
        let uart_ref = &uart;
        let display = &display;

        s.spawn(move || vmm_thread(vm_ref, handle_tx, uart_ref, display, input_rx));

        let handle = handle_rx.recv().unwrap();

        let stdin_handle = handle.clone();
        s.spawn(move || stdin_thread(vm_ref, stdin_handle, uart_ref));

        // has to run in main thread
        window_thread(vm_ref, handle, display, input_tx);
    });

    Ok(())
}
