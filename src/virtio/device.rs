use crate::virtio::virtq::VirtQueue;
use applevisor::memory::Memory;

pub trait Device {
    fn id(&self) -> u32;
    fn features(&self) -> u64;
    fn config(&self) -> u32;
    fn num_queues(&self) -> u16;
    fn handle_request(&mut self, queue: &VirtQueue, head_idx: u16, mem: &mut Memory) -> u32;
}
