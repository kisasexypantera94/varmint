use std::{
    file,
    fs::File,
    io::{self, Read},
};

use applevisor::prelude::*;

const KB: usize = 1024;
const MB: usize = KB * 1024;
const GB: usize = MB * 1024;
const RAM_START: usize = 0x40000000;
const _RAM_START_GB: usize = RAM_START / GB;

const INITRD_START: usize = 0x48000000;
const INITRD_OFFSET_MB: usize = (INITRD_START - RAM_START) / MB;

const DTB_START: usize = 0x4F000000;

fn read_file(path: &str) -> std::io::Result<Vec<u8>> {
    let mut f = File::open("./artifacts/guest.dtb")?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;

    return Ok(buf);
}

fn main() -> Result<()> {
    let dtb = read_file("./artifacts/guest.dtb").unwrap();
    let initrd = read_file("./artifacts/debian-arm64/initrd.gz").unwrap();
    let image = read_file("./artifacts/debian-arm64/Image").unwrap();

    // Creates a new virtual machine. There can be one, and only one, per process. Operations
    // on the virtual machine remains possible as long as this object is valid.
    let vm = VirtualMachine::new()?;

    // Creates a new virtual CPU. This object abstracts operations that can be performed on
    // CPUs, such as starting and stopping them, changing their registers, etc.
    let vcpu = vm.vcpu_create()?;

    // Enables debug features for the hypervisor. This is optional, but it might be required
    // for certain features to work, such as breakpoints.
    vcpu.set_trap_debug_exceptions(true)?;
    vcpu.set_trap_debug_reg_accesses(true)?;

    // Creates a mapping object that represents a 0x1000-byte physical memory range.
    let mut mem = vm.memory_create(0x1000)?;

    // This mapping needs to be mapped to effectively allocate physical memory for the guest.
    // Here we map the region at address 0x4000 and set the permissions to Read-Write-Execute.
    mem.map(0x4000, MemPerms::RWX)?;
    // Writes a `mov x0, #0x42` instruction at address 0x4000.
    mem.write_u32(0x4000, 0xd2800840)?;
    // Writes a `brk #0` instruction at address 0x4004.
    mem.write_u32(0x4004, 0xd4200000)?;

    // Sets PC to 0x4000.
    vcpu.set_reg(Reg::PC, 0x4000)?;

    // Starts the Vcpu. It will execute our mov and breakpoint instructions before stopping.
    loop {
        println!("PC was {:?}", vcpu.get_reg(Reg::PC)?);
        vcpu.run()?;
        let exit_info = vcpu.get_exit_info();
        println!("PC is {:?}", vcpu.get_reg(Reg::PC)?);

        return Ok(());

        // The *exit information* can be used to used to retrieve different pieces of
        // information about the CPU exit status (e.g. exception type, fault address, etc.).

        // If everything went as expected, the value in X0 is 0x42...
        assert_eq!(vcpu.get_reg(Reg::X0), Ok(0x42));
        // ... the vcpu has stopped because of an exception ...
        assert_eq!(exit_info.reason, ExitReason::EXCEPTION);
        // ... and the exception syndrome corresponds to a breakpoint exception (which would
        // have been a different value without the call to `set_trap_debug_exceptions()`).
        assert_eq!(exit_info.exception.syndrome >> 26, 0b111100);
    }
}
