use crate::{
    memory::GuestMemory,
    virtio::{chain::ChainData, virtq::Queue},
};

const INT_VRING: u32 = 1 << 0;
const INT_CONFIG: u32 = 1 << 1;

pub enum ChainAction {
    Complete(u32),
    Deferred,
}

#[derive(Debug, Copy, Clone)]
pub struct ChainToken {
    pub queue_idx: usize,
    pub head_idx: u16,
}

pub struct AvailableChain {
    pub data: ChainData,
    pub token: ChainToken,
}

pub struct DeviceContext<'a> {
    queues: &'a mut [Queue],
    interrupt_status: &'a mut u32,
    mem: &'a GuestMemory,
}

impl<'a> DeviceContext<'a> {
    pub fn new(queues: &'a mut [Queue], interrupt_status: &'a mut u32, mem: &'a GuestMemory) -> Self {
        Self {
            queues,
            interrupt_status,
            mem,
        }
    }

    pub fn mem(&self) -> &GuestMemory {
        self.mem
    }

    pub fn pop_chain(&mut self, queue_idx: usize) -> Option<AvailableChain> {
        loop {
            let queue = self.queues.get_mut(queue_idx)?;
            let head_idx = queue.pop_chain(self.mem)?;

            match queue.collect_chain(head_idx, self.mem) {
                Some(data) => {
                    return Some(AvailableChain {
                        data,
                        token: ChainToken { queue_idx, head_idx },
                    });
                }
                None => {
                    queue.push_used(self.mem, head_idx, 0);
                    *self.interrupt_status |= INT_VRING;
                }
            }
        }
    }

    pub fn complete(&mut self, token: ChainToken, written: u32) {
        let Some(queue) = self.queues.get_mut(token.queue_idx) else {
            return;
        };

        if queue.ready {
            queue.push_used(self.mem, token.head_idx, written);
            *self.interrupt_status |= INT_VRING;
        }
    }

    pub fn deliver(&mut self, queue_idx: usize, parts: &[&[u8]]) -> bool {
        let Some(chain) = self.pop_chain(queue_idx) else {
            return false;
        };

        let written = chain.data.write_parts(parts, self.mem);
        self.complete(chain.token, written);
        true
    }

    pub fn config_changed(&mut self) {
        *self.interrupt_status |= INT_CONFIG;
    }
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
    fn queue_notified(&mut self, queue_idx: usize, ctx: &mut DeviceContext<'_>);
    fn reset(&mut self) {}

    fn shared_memory_region(&self, _id: u32) -> Option<ShmRegion> {
        None
    }
}

pub trait ExternalEventHandler {
    type Event<'a>;

    fn on_event(&mut self, event: Self::Event<'_>, ctx: &mut DeviceContext<'_>);
}
