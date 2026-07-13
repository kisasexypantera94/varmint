use crate::machine::*;
use applevisor::prelude::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SecondaryStart {
    pub(crate) entry_point: u64,
    pub(crate) context_id: u64,
}

pub(crate) enum PsciAction {
    Continue,
    CpuOff,
}

pub(crate) fn handle_psci_hvc(
    vcpu: &Vcpu,
    vcpu_id: usize,
    secondary_boot_txs: &[SyncSender<SecondaryStart>],
    secondary_online: &[AtomicBool],
) -> Result<PsciAction> {
    let function_id = vcpu.get_reg(Reg::X0)?;

    let ret = match function_id {
        PSCI_VERSION => PSCI_VERSION_0_2,
        PSCI_CPU_ON_64 => {
            let target_mpidr = vcpu.get_reg(Reg::X1)?;
            let entry_point = vcpu.get_reg(Reg::X2)?;
            let context_id = vcpu.get_reg(Reg::X3)?;

            match secondary_index_for_mpidr(target_mpidr) {
                Some(index) => {
                    if secondary_online[index].swap(true, Ordering::SeqCst) {
                        PSCI_ALREADY_ON
                    } else {
                        let start = SecondaryStart {
                            entry_point,
                            context_id,
                        };
                        match secondary_boot_txs[index].send(start) {
                            Ok(()) => PSCI_SUCCESS,
                            Err(_) => {
                                secondary_online[index].store(false, Ordering::SeqCst);
                                PSCI_DENIED
                            }
                        }
                    }
                }
                None => PSCI_INVALID_PARAMETERS,
            }
        }
        PSCI_CPU_OFF => {
            if vcpu_id >= FIRST_SECONDARY_VCPU_ID {
                let index = vcpu_id - FIRST_SECONDARY_VCPU_ID;
                secondary_online[index].store(false, Ordering::SeqCst);
                vcpu.set_reg(Reg::X0, PSCI_SUCCESS)?;
                return Ok(PsciAction::CpuOff);
            }
            PSCI_DENIED
        }
        PSCI_SYSTEM_OFF | PSCI_SYSTEM_RESET => {
            eprintln!("PSCI system off/reset requested");
            PSCI_SUCCESS
        }
        _ => PSCI_NOT_SUPPORTED,
    };

    vcpu.set_reg(Reg::X0, ret)?;

    Ok(PsciAction::Continue)
}
