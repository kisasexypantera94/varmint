#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysReg {
    op0: u64,
    op1: u64,
    crn: u64,
    crm: u64,
    op2: u64,
}

// https://developer.arm.com/documentation/ddi0601/2026-03/AArch64-Registers/ID-AA64ISAR2-EL1--AArch64-Instruction-Set-Attribute-Register-2?lang=en
pub const ID_AA64ISAR2_EL1: SysReg = SysReg {
    op0: 0b11,
    op1: 0b000,
    crn: 0b0000,
    crm: 0b0110,
    op2: 0b010,
};

pub const MDSCR_EL1: SysReg = SysReg {
    op0: 2,
    op1: 0,
    crn: 0,
    crm: 2,
    op2: 2,
};

pub const OSDLR_EL1: SysReg = SysReg {
    op0: 2,
    op1: 0,
    crn: 1,
    crm: 3,
    op2: 4,
};

pub const OSLAR_EL1: SysReg = SysReg {
    op0: 2,
    op1: 0,
    crn: 1,
    crm: 0,
    op2: 4,
};

pub fn decode_sysreg(esr: u64) -> (SysReg, u64, bool) {
    let iss = esr & 0x01ff_ffff;

    let op0 = (iss >> 20) & 0b11;
    let op2 = (iss >> 17) & 0b111;
    let op1 = (iss >> 14) & 0b111;
    let crn = (iss >> 10) & 0b1111;
    let rt = (iss >> 5) & 0b11111;
    let crm = (iss >> 1) & 0b1111;

    // For EC=0x18: 1 = MRS/read, 0 = MSR/write.
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
