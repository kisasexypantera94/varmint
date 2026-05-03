use applevisor::prelude::*;

pub fn reg_from_rt(rt: u64) -> Option<Reg> {
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
        31 => None, // XZR/WZR: zero register, not a real stored register
        _ => None,
    }
}

pub fn set_rt(vcpu: &Vcpu, rt: u64, value: u64) -> Result<()> {
    if let Some(reg) = reg_from_rt(rt) {
        vcpu.set_reg(reg, value)?;
    }

    Ok(())
}

pub fn get_rt(vcpu: &Vcpu, rt: u64) -> Result<u64> {
    Ok(match reg_from_rt(rt) {
        Some(reg) => vcpu.get_reg(reg)?,
        None => 0, // XZR/WZR
    })
}
