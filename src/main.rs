use applevisor::prelude::*;

use std::fs::File;
use std::io::Read;

mod helpers;
mod linux;
mod sys_reg;
mod uart;

const RAM_START: u64 = 0x40000000;
const RAM_SIZE: usize = 0x20000000;
const KERNEL_TEXT_OFFSET: u64 = 0x0;
const IMAGE_START: u64 = RAM_START + KERNEL_TEXT_OFFSET;
const INITRD_START: u64 = 0x48000000;
const DTB_START: u64 = 0x4F000000;
const UART_START: u64 = 0x09000000;
const UART_SIZE: u64 = 0x1000;

const PSTATE_EL1H_DAIF_MASKED: u64 = 0x3c5;

fn read_file(path: &str) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;

    return Ok(buf);
}

fn main() -> Result<()> {
    let image = read_file("./artifacts/debian-arm64/Image").unwrap();
    let initrd = read_file("./artifacts/debian-arm64/initrd.gz").unwrap();
    let dtb = read_file("./artifacts/guest.dtb").unwrap();

    let image_header = linux::parse_image_header(&image).unwrap();

    println!("Image header: {:?}", image_header);

    let vm = VirtualMachine::new()?;

    let vcpu = vm.vcpu_create()?;

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

    loop {
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
                        if handle_sysreg_trap(&vcpu, esr_el2_like, pc)? {
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

                        let offset = phys_addr - UART_START;

                        if (UART_START..UART_START + UART_SIZE).contains(&phys_addr) {
                            if is_write {
                                let value = match helpers::reg_from_rt(rt) {
                                    Some(reg) => vcpu.get_reg(reg)?,
                                    None => 0,
                                };

                                uart::pl011_write(offset, value);
                            } else {
                                let value = uart::pl011_read(offset);

                                if let Some(reg) = helpers::reg_from_rt(rt) {
                                    vcpu.set_reg(reg, value)?;
                                }
                            }

                            vcpu.set_reg(Reg::PC, pc + 4)?;
                            continue;
                        }
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
            _ => panic!("unexpected exit: {:?}", exit),
        }
    }
}

fn handle_sysreg_trap(vcpu: &Vcpu, esr: u64, pc: u64) -> Result<bool> {
    let (sysreg, rt, is_read) = sys_reg::decode_sysreg(esr);

    println!(
        "sysreg trap: {:?}, rt={}, {}",
        sysreg,
        rt,
        if is_read { "read/MRS" } else { "write/MSR" }
    );

    match (sysreg, is_read) {
        (sys_reg::ID_AA64ISAR2_EL1, true) => {
            // Conservative value: expose no optional ISAR2 features.
            helpers::set_rt(vcpu, rt, 0)?;
        }

        (sys_reg::MDSCR_EL1, true) => {
            // Debug control register. For bring-up, report debug disabled.
            helpers::set_rt(vcpu, rt, 0)?;
        }

        (sys_reg::MDSCR_EL1, false) => {
            // Linux may try to configure debug state. Ignore for now.
            let value = helpers::get_rt(vcpu, rt)?;
            println!("ignored MDSCR_EL1 write: 0x{value:x}");
        }

        _ => {
            return Ok(false);
        }
    }

    vcpu.set_reg(Reg::PC, pc + 4)?;
    Ok(true)
}
