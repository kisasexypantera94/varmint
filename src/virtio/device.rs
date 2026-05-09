use crate::virtio::virtq::VirtQueue;
use applevisor::memory::Memory;

pub trait Device {
    fn id(&self) -> u32;
    fn features(&self) -> u64;
    fn config(&self) -> u32;
    fn num_queues(&self) -> u16;
    fn process_queue(&mut self, queue: &mut VirtQueue, mem: &mut Memory) -> bool;
}
