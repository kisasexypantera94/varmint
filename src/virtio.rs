//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html

mod blk;
mod common;
mod device;
pub mod gpu;
mod mmio;
mod net;
mod virtq;

pub use blk::Blk;
pub use gpu::Gpu;
pub use net::Net;
pub type MmioTransport<D> = mmio::Transport<D>;
