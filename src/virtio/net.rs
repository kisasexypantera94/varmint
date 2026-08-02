use crate::virtio::{
    common,
    device::{Device, DeviceContext, ExternalEventHandler},
};
use num_enum::TryFromPrimitive;
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes};

mod feature {
    pub const MAC: u64 = 1 << 5;
}

const DEVICE_ID: u32 = 1;

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(usize)]
enum QueueType {
    Rx = 0,
    Tx = 1,
}

#[derive(IntoBytes, FromBytes, Immutable)]
#[repr(C, packed)]
struct NetHeader {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    gso_size: u16,
    csum_start: u16,
    csum_offset: u16,
    num_buffers: u16,
}

pub struct Net {
    mac: [u8; 6],
    backend: crate::net::Backend,
}

impl Net {
    pub fn new(mac: [u8; 6], backend: crate::net::Backend) -> Net {
        Net { mac, backend }
    }

    fn handle_tx(&self, chain: &crate::virtio::chain::ChainData, mem: &crate::memory::GuestMemory) -> u32 {
        const HEADER_LEN: usize = size_of::<NetHeader>();

        let total = chain.readable_len();
        if total < HEADER_LEN {
            eprintln!("TX: chain too short, {} bytes", total);
            return 0;
        }

        let mut eth = vec![0u8; total - HEADER_LEN];
        if chain.read_at(HEADER_LEN, &mut eth, mem).is_none() {
            eprintln!("TX: failed to read frame from guest memory");
            return 0;
        }

        self.backend.write(&eth);
        0
    }

    fn plain_rx_hdr() -> NetHeader {
        let mut hdr = NetHeader::new_zeroed();
        hdr.num_buffers = 1;
        hdr
    }
}

impl Device for Net {
    fn id(&self) -> u32 {
        DEVICE_ID
    }

    fn features(&self) -> u64 {
        common::feature::VERSION_1 | feature::MAC
    }

    fn read_config(&self, offset: u64, data: &mut [u8]) {
        let offset = offset as usize;
        data.copy_from_slice(&self.mac[offset..offset + data.len()]);
    }

    fn num_queues(&self) -> u16 {
        2
    }

    fn queue_notified(&mut self, queue_idx: usize, ctx: &mut DeviceContext<'_>) {
        match QueueType::try_from(queue_idx).unwrap() {
            QueueType::Rx => {}
            QueueType::Tx => {
                while let Some(chain) = ctx.pop_chain(queue_idx) {
                    let written = self.handle_tx(&chain.data, ctx.mem());
                    ctx.complete(chain.token, written);
                }
            }
        }
    }

    fn reset(&mut self) {}
}

impl ExternalEventHandler for Net {
    type Event<'a> = &'a [u8];

    fn on_event(&mut self, frame: Self::Event<'_>, ctx: &mut DeviceContext<'_>) {
        let header = Self::plain_rx_hdr();
        let _ = ctx.deliver(QueueType::Rx as usize, &[header.as_bytes(), frame]);
    }
}
