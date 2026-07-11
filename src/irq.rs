use applevisor::prelude::*;

pub struct IrqLine {
    vm: VirtualMachineInstance<GicEnabled>,
    intid: u32,
    level: bool,
}

impl IrqLine {
    pub fn new(vm: &VirtualMachineInstance<GicEnabled>, intid: u32) -> Self {
        Self {
            vm: vm.clone(),
            intid,
            level: false,
        }
    }

    pub fn set(&mut self, level: bool) -> bool {
        if level == self.level {
            return false;
        }

        let rising = level;
        self.vm.gic_set_spi(self.intid, level).unwrap();
        self.level = level;
        rising
    }
}
