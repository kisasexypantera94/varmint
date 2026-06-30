use applevisor::{
    error::{HypervisorError, Result},
    memory::Memory,
};
use std::{ffi::c_void, ptr};

#[derive(Debug)]
pub struct GuestMemory {
    inner: Memory,
}

unsafe impl Send for GuestMemory {}
unsafe impl Sync for GuestMemory {}

impl GuestMemory {
    pub fn new(inner: Memory) -> Self {
        Self { inner }
    }

    pub fn map(&mut self, guest_addr: u64, perms: applevisor::memory::MemPerms) -> Result<()> {
        self.inner.map(guest_addr, perms)
    }

    pub fn guest_addr(&self) -> Option<u64> {
        self.inner.guest_addr()
    }

    pub fn host_addr(&self) -> *mut u8 {
        self.inner.host_addr()
    }

    pub fn size(&self) -> usize {
        self.inner.size()
    }

    pub fn checked_host_addr(&self, guest_addr: u64, len: usize) -> Result<*mut u8> {
        let mapping_guest_addr = self.inner.guest_addr().ok_or(HypervisorError::Error)?;

        if guest_addr < mapping_guest_addr {
            return Err(HypervisorError::BadArgument);
        }

        let end = guest_addr.checked_add(len as u64).ok_or(HypervisorError::BadArgument)?;

        let mapping_end = mapping_guest_addr
            .checked_add(self.inner.size() as u64)
            .ok_or(HypervisorError::BadArgument)?;

        if end > mapping_end {
            return Err(HypervisorError::BadArgument);
        }

        let offset = guest_addr - mapping_guest_addr;
        Ok(unsafe { self.inner.host_addr().add(offset as usize) })
    }

    pub fn read(&self, guest_addr: u64, data: &mut [u8]) -> Result<()> {
        self.inner.read(guest_addr, data)
    }

    pub fn write(&self, guest_addr: u64, data: &[u8]) -> Result<()> {
        let host_addr = self.checked_host_addr(guest_addr, data.len())?;

        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr() as *const c_void, host_addr as *mut c_void, data.len());
        }

        Ok(())
    }

    pub fn read_u16(&self, guest_addr: u64) -> Result<u16> {
        self.inner.read_u16(guest_addr)
    }

    pub fn read_u32(&self, guest_addr: u64) -> Result<u32> {
        self.inner.read_u32(guest_addr)
    }

    pub fn read_u64(&self, guest_addr: u64) -> Result<u64> {
        self.inner.read_u64(guest_addr)
    }

    pub fn write_u8(&self, guest_addr: u64, value: u8) -> Result<()> {
        self.write(guest_addr, &[value])
    }

    pub fn write_u16(&self, guest_addr: u64, value: u16) -> Result<()> {
        self.write(guest_addr, &value.to_le_bytes())
    }

    pub fn write_u32(&self, guest_addr: u64, value: u32) -> Result<()> {
        self.write(guest_addr, &value.to_le_bytes())
    }
}
