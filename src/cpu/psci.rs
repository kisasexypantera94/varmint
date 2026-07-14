use super::{SecondaryCpus, StartResult};
use applevisor::prelude::*;

const PSCI_VERSION: u64 = 0x84000000;
const PSCI_CPU_OFF: u64 = 0x84000002;
const PSCI_CPU_ON_64: u64 = 0xC4000003;
const PSCI_SYSTEM_OFF: u64 = 0x84000008;
const PSCI_SYSTEM_RESET: u64 = 0x84000009;
const PSCI_VERSION_0_2: u64 = 0x00000002;
const PSCI_SUCCESS: u64 = 0;
const PSCI_NOT_SUPPORTED: u64 = -1i64 as u64;
const PSCI_INVALID_PARAMETERS: u64 = -2i64 as u64;
const PSCI_DENIED: u64 = -3i64 as u64;
const PSCI_ALREADY_ON: u64 = -4i64 as u64;

pub enum Action {
    Continue,
    CpuOff,
}

pub fn handle_hvc(vcpu: &Vcpu, vcpu_id: usize, secondaries: &SecondaryCpus) -> Result<Action> {
    let function_id = vcpu.get_reg(Reg::X0)?;

    let ret = match function_id {
        PSCI_VERSION => PSCI_VERSION_0_2,
        PSCI_CPU_ON_64 => {
            let target_mpidr = vcpu.get_reg(Reg::X1)?;
            let entry_point = vcpu.get_reg(Reg::X2)?;
            let context_id = vcpu.get_reg(Reg::X3)?;

            match secondaries.start(target_mpidr, entry_point, context_id) {
                StartResult::Started => PSCI_SUCCESS,
                StartResult::InvalidCpu => PSCI_INVALID_PARAMETERS,
                StartResult::AlreadyOn => PSCI_ALREADY_ON,
                StartResult::Unavailable => PSCI_DENIED,
            }
        }
        PSCI_CPU_OFF => {
            if secondaries.stop(vcpu_id) {
                vcpu.set_reg(Reg::X0, PSCI_SUCCESS)?;
                return Ok(Action::CpuOff);
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
    Ok(Action::Continue)
}
