use crate::{memory::GuestMemory, virtio::chain::ChainData};

pub enum ChainAction {
    Complete(u32),
    Deferred,
}

pub struct ChainToken {
    pub queue_idx: usize,
    pub head_idx: u16,
}

pub struct ShmRegion {
    pub base: u64,
    pub len: u64,
}

pub trait Device {
    fn id(&self) -> u32;
    fn features(&self) -> u64;
    fn num_queues(&self) -> u16;
    fn read_config(&self, offset: u64, data: &mut [u8]);
    fn write_config(&mut self, _offset: u64, _data: &[u8]) {}

    fn process_chain(
        &mut self,
        queue_idx: usize,
        chain: &ChainData,
        token: ChainToken,
        mem: &GuestMemory,
    ) -> ChainAction;

    fn delivery_queues(&self) -> &[u16] {
        &[]
    }

    fn pop_external(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn reset(&mut self) {}

    fn shared_memory_region(&self, _id: u32) -> Option<ShmRegion> {
        None
    }
}

pub enum Effect<'a> {
    Deliver { queue_idx: usize, parts: &'a [&'a [u8]] },
    Complete { token: ChainToken, written: u32 },
    Config,
}

pub trait ExternalEventHandler {
    type Event<'a>;

    fn on_event(&mut self, event: Self::Event<'_>, emit: impl FnMut(Effect));
}
