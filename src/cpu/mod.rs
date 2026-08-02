mod psci;
mod sysreg;

use crate::{devices::Devices, memory::GuestMemory};
use applevisor::prelude::*;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
};

const BOOT_VCPU_ID: usize = 0;
const FIRST_SECONDARY_VCPU_ID: usize = 1;
const PSTATE_EL1H_DAIF_MASKED: u64 = 0x3c5;
const ESR_EC_HVC_AARCH64: u64 = 0x16;

#[derive(Debug, Clone, Copy)]
struct SecondaryStart {
    entry_point: u64,
    context_id: u64,
}

enum StartResult {
    Started,
    InvalidCpu,
    AlreadyOn,
    Unavailable,
}

struct SecondaryCpu {
    boot_tx: SyncSender<SecondaryStart>,
    online: AtomicBool,
}

struct SecondaryCpus {
    cpus: Vec<SecondaryCpu>,
}

pub struct CpuRuntime {
    boot_vcpu: Vcpu,
    secondaries: SecondaryCpus,
    secondary_workers: Vec<SecondaryWorker>,
}

struct SecondaryWorker {
    vcpu_id: usize,
    boot_rx: Receiver<SecondaryStart>,
}

impl CpuRuntime {
    pub fn new(vm: &VirtualMachineInstance<GicEnabled>, entry_point: u64, x0: u64, vcpus: usize) -> Result<Self> {
        let boot_vcpu = vm.vcpu_create()?;
        configure_vcpu(&boot_vcpu, BOOT_VCPU_ID)?;
        set_entry_state(&boot_vcpu, entry_point, x0)?;

        let (secondaries, secondary_workers) = SecondaryCpus::new(vcpus);

        Ok(Self {
            boot_vcpu,
            secondaries,
            secondary_workers,
        })
    }

    pub fn run(self, vm: &VirtualMachineInstance<GicEnabled>, mem: &GuestMemory, devices: &Devices) -> Result<()> {
        let Self {
            boot_vcpu,
            secondaries,
            secondary_workers,
        } = self;

        thread::scope(|scope| -> Result<()> {
            for worker in secondary_workers {
                let secondaries = &secondaries;
                scope.spawn(move || worker.run(vm, mem, devices, secondaries).unwrap());
            }

            run_loop(&boot_vcpu, BOOT_VCPU_ID, mem, devices, &secondaries)
        })
    }
}

impl SecondaryCpus {
    fn new(vcpus: usize) -> (Self, Vec<SecondaryWorker>) {
        let mut cpus = Vec::new();
        let mut workers = Vec::new();

        for vcpu_id in FIRST_SECONDARY_VCPU_ID..vcpus {
            let (boot_tx, boot_rx) = sync_channel(1);
            cpus.push(SecondaryCpu {
                boot_tx,
                online: AtomicBool::new(false),
            });
            workers.push(SecondaryWorker { vcpu_id, boot_rx });
        }

        (Self { cpus }, workers)
    }

    fn start(&self, mpidr: u64, entry_point: u64, context_id: u64) -> StartResult {
        let Some(cpu) = self.cpu_for_mpidr(mpidr) else {
            return StartResult::InvalidCpu;
        };

        if cpu.online.swap(true, Ordering::SeqCst) {
            return StartResult::AlreadyOn;
        }

        if cpu
            .boot_tx
            .send(SecondaryStart {
                entry_point,
                context_id,
            })
            .is_err()
        {
            cpu.online.store(false, Ordering::SeqCst);
            return StartResult::Unavailable;
        }

        StartResult::Started
    }

    fn stop(&self, vcpu_id: usize) -> bool {
        let Some(cpu) = vcpu_id
            .checked_sub(FIRST_SECONDARY_VCPU_ID)
            .and_then(|index| self.cpus.get(index))
        else {
            return false;
        };

        cpu.online.store(false, Ordering::SeqCst);
        true
    }

    fn cpu_for_mpidr(&self, mpidr: u64) -> Option<&SecondaryCpu> {
        let vcpu_id = (mpidr & 0xff) as usize;
        vcpu_id
            .checked_sub(FIRST_SECONDARY_VCPU_ID)
            .and_then(|index| self.cpus.get(index))
    }
}

impl SecondaryWorker {
    fn run(
        self,
        vm: &VirtualMachineInstance<GicEnabled>,
        mem: &GuestMemory,
        devices: &Devices,
        secondaries: &SecondaryCpus,
    ) -> Result<()> {
        let vcpu = vm.vcpu_create()?;
        configure_vcpu(&vcpu, self.vcpu_id)?;
        while let Ok(start) = self.boot_rx.recv() {
            set_entry_state(&vcpu, start.entry_point, start.context_id)?;
            run_loop(&vcpu, self.vcpu_id, mem, devices, secondaries)?;
        }

        Ok(())
    }
}

fn configure_vcpu(vcpu: &Vcpu, vcpu_id: usize) -> Result<()> {
    vcpu.set_sys_reg(SysReg::ACTLR_EL1, 1 << 1)?; // enable TSO
    vcpu.set_sys_reg(SysReg::MPIDR_EL1, mpidr_for_vcpu(vcpu_id))?;
    Ok(())
}

fn set_entry_state(vcpu: &Vcpu, entry_point: u64, x0: u64) -> Result<()> {
    vcpu.set_reg(Reg::CPSR, PSTATE_EL1H_DAIF_MASKED)?; // start in EL1
    vcpu.set_reg(Reg::PC, entry_point)?;
    vcpu.set_reg(Reg::X0, x0)?;
    vcpu.set_reg(Reg::X1, 0)?;
    vcpu.set_reg(Reg::X2, 0)?;
    vcpu.set_reg(Reg::X3, 0)?;
    Ok(())
}

fn mpidr_for_vcpu(vcpu_id: usize) -> u64 {
    vcpu_id as u64
}

fn reg_from_rt(rt: u64) -> Option<Reg> {
    match rt {
        0 => Some(Reg::X0),
        1 => Some(Reg::X1),
        2 => Some(Reg::X2),
        3 => Some(Reg::X3),
        4 => Some(Reg::X4),
        5 => Some(Reg::X5),
        6 => Some(Reg::X6),
        7 => Some(Reg::X7),
        8 => Some(Reg::X8),
        9 => Some(Reg::X9),
        10 => Some(Reg::X10),
        11 => Some(Reg::X11),
        12 => Some(Reg::X12),
        13 => Some(Reg::X13),
        14 => Some(Reg::X14),
        15 => Some(Reg::X15),
        16 => Some(Reg::X16),
        17 => Some(Reg::X17),
        18 => Some(Reg::X18),
        19 => Some(Reg::X19),
        20 => Some(Reg::X20),
        21 => Some(Reg::X21),
        22 => Some(Reg::X22),
        23 => Some(Reg::X23),
        24 => Some(Reg::X24),
        25 => Some(Reg::X25),
        26 => Some(Reg::X26),
        27 => Some(Reg::X27),
        28 => Some(Reg::X28),
        29 => Some(Reg::X29),
        30 => Some(Reg::X30),
        31 => None,
        _ => None,
    }
}

fn read_rt(vcpu: &Vcpu, rt: u64) -> Result<u64> {
    Ok(match reg_from_rt(rt) {
        Some(reg) => vcpu.get_reg(reg)?,
        None => 0,
    })
}

fn write_rt(vcpu: &Vcpu, rt: u64, value: u64) -> Result<()> {
    if let Some(reg) = reg_from_rt(rt) {
        vcpu.set_reg(reg, value)?;
    }
    Ok(())
}

fn run_loop(
    vcpu: &Vcpu,
    vcpu_id: usize,
    mem: &GuestMemory,
    devices: &Devices,
    secondaries: &SecondaryCpus,
) -> Result<()> {
    loop {
        vcpu.run()?;
        let exit = vcpu.get_exit_info();

        match exit.reason {
            ExitReason::EXCEPTION => {
                let esr = exit.exception.syndrome;
                let phys_addr = exit.exception.physical_address;
                let pc = vcpu.get_reg(Reg::PC)?;
                let ec = (esr >> 26) & 0b111111;

                match ec {
                    ESR_EC_HVC_AARCH64 => match psci::handle_hvc(vcpu, vcpu_id, secondaries)? {
                        psci::Action::Continue => continue,
                        psci::Action::CpuOff => return Ok(()),
                    },

                    0x18 => {
                        if sysreg::handle_trap(vcpu, esr, pc)? {
                            continue;
                        }

                        let (sysreg, rt, is_read) = sysreg::decode(esr);
                        panic!(
                            "unhandled sysreg trap: {:?}, rt={}, {}, esr=0x{:x}, pc=0x{:x}",
                            sysreg,
                            rt,
                            if is_read { "read/MRS" } else { "write/MSR" },
                            esr,
                            pc
                        );
                    }

                    0x24 | 0x25 => {
                        let is_write = ((esr >> 6) & 1) == 1;
                        let rt = (esr >> 16) & 0b11111;
                        let size = 1usize << ((esr >> 22) & 0b11);

                        let value = if is_write { read_rt(vcpu, rt)? as u32 as u64 } else { 0 };
                        let Ok(read_value) = devices.handle_mmio(phys_addr, is_write, size, value, mem) else {
                            panic!(
                                "unhandled data abort trap: ec={}, rt={}, {}, esr=0x{:x}, pc=0x{:x}, addr=0x{:x}",
                                ec,
                                rt,
                                if is_write { "write" } else { "read" },
                                esr,
                                pc,
                                phys_addr,
                            );
                        };

                        if let Some(read_value) = read_value {
                            write_rt(vcpu, rt, read_value)?;
                        }

                        vcpu.set_reg(Reg::PC, pc + 4)?;
                    }

                    0x20 | 0x21 => {
                        panic!(
                            "instruction abort: fault=0x{:x}, esr=0x{:x}, pc=0x{:x}",
                            exit.exception.physical_address, esr, pc
                        );
                    }

                    _ => {
                        panic!("unexpected exception ec=0x{:x}, esr=0x{:x}, pc=0x{:x}", ec, esr, pc);
                    }
                }
            }
            ExitReason::CANCELED => (),
            _ => eprintln!("unexpected exit reason: {:?}", exit),
        }
    }
}
