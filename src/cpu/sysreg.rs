use super::{read_rt, write_rt};
use applevisor::{
    error::Result,
    vcpu::{Reg, Vcpu},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysReg {
    op0: u64,
    op1: u64,
    crn: u64,
    crm: u64,
    op2: u64,
}

const ID_AA64ISAR2_EL1: SysReg = SysReg {
    op0: 0b11,
    op1: 0b000,
    crn: 0b0000,
    crm: 0b0110,
    op2: 0b010,
};

const MDSCR_EL1: SysReg = SysReg {
    op0: 2,
    op1: 0,
    crn: 0,
    crm: 2,
    op2: 2,
};

const OSDLR_EL1: SysReg = SysReg {
    op0: 2,
    op1: 0,
    crn: 1,
    crm: 3,
    op2: 4,
};

const OSLAR_EL1: SysReg = SysReg {
    op0: 2,
    op1: 0,
    crn: 1,
    crm: 0,
    op2: 4,
};

pub fn decode(esr: u64) -> (SysReg, u64, bool) {
    let iss = esr & 0x01ff_ffff;

    let op0 = (iss >> 20) & 0b11;
    let op2 = (iss >> 17) & 0b111;
    let op1 = (iss >> 14) & 0b111;
    let crn = (iss >> 10) & 0b1111;
    let rt = (iss >> 5) & 0b11111;
    let crm = (iss >> 1) & 0b1111;
    let is_read = (iss & 1) == 1;

    (
        SysReg {
            op0,
            op1,
            crn,
            crm,
            op2,
        },
        rt,
        is_read,
    )
}

pub fn handle_trap(vcpu: &Vcpu, esr: u64, pc: u64) -> Result<bool> {
    let (sysreg, rt, is_read) = decode(esr);

    eprintln!(
        "sysreg trap: {:?}, rt={}, {}",
        sysreg,
        rt,
        if is_read { "read/MRS" } else { "write/MSR" }
    );

    match (sysreg, is_read) {
        (ID_AA64ISAR2_EL1, true) | (MDSCR_EL1, true) | (OSDLR_EL1, true) | (OSLAR_EL1, true) => {
            write_rt(vcpu, rt, 0)?;
        }
        (MDSCR_EL1, false) => {
            let value = read_rt(vcpu, rt)?;
            eprintln!("ignored MDSCR_EL1 write: 0x{value:x}");
        }
        (OSDLR_EL1, false) => {
            let value = read_rt(vcpu, rt)?;
            eprintln!("ignored OSDLR_EL1 write: 0x{value:x}");
        }
        (OSLAR_EL1, false) => {
            let value = read_rt(vcpu, rt)?;
            eprintln!("ignored OSLAR_EL1 write: 0x{value:x}");
        }
        _ => return Ok(false),
    }

    vcpu.set_reg(Reg::PC, pc + 4)?;
    Ok(true)
}
