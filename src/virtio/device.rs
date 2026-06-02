use crate::virtio::virtq::{Completion, Queue};
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
    ) -> Option<u32>;

    fn handle_external(
        &mut self,
        queues: &[Queue],
        data: &[u8],
        mem: &mut Memory,
    ) -> Option<Completion>;

    fn pop_external(&mut self) -> Option<Vec<u8>>;
}
