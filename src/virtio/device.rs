use crate::virtio::virtq::{Completion, Queue};
use applevisor::memory::Memory;

pub trait Device {
    fn id(&self) -> u32;
    fn features(&self) -> u64;
    fn num_queues(&self) -> u16;
    fn read_config(&self, offset: u64, data: &mut [u8]);
    fn write_config(&mut self, _offset: u64, _data: &[u8]) {}

    fn process_chain(
        &mut self,
        queue_idx: usize,
        queue: &Queue,
        head_idx: u16,
        mem: &mut Memory,
    ) -> Option<u32>;

    fn handle_external(
        &mut self,
        _queues: &[Queue],
        _data: &[u8],
        _mem: &mut Memory,
    ) -> Option<Completion> {
        None
    }

    fn pop_external(&mut self) -> Option<Vec<u8>> {
        None
    }
}
