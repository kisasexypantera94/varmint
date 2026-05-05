use applevisor::prelude::*;

pub struct IrqLine {
    intid: u32,
    last_level: bool,
}

impl IrqLine {
    pub fn new(intid: u32, init_level: bool) -> IrqLine {
        IrqLine {
            intid: intid,
            last_level: init_level,
        }
    }

    pub fn sync(&mut self, vm: &VirtualMachineInstance<GicEnabled>, level: bool) -> Result<()> {
        if level != self.last_level {
            self.last_level = level;
            vm.gic_set_spi(self.intid, level)?;
        }

        Ok(())
    }
}
