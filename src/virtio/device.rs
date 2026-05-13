use crate::virtio::virtq::Queue;
use applevisor::memory::Memory;

pub trait Device {
    fn id(&self) -> u32;
    fn features(&self) -> u64;
    fn config(&self, offset: u64) -> u32;
    fn num_queues(&self) -> u16;
    fn process_chain(
        &mut self,
        queue_idx: usize,
        queue: &Queue,
        head_idx: u16,
        mem: &mut Memory,
    ) -> u32;
}
