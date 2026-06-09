//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html

mod blk;
mod common;
mod device;
pub mod gpu;
pub mod input;
mod mmio;
mod net;
mod virtq;

pub use blk::Blk;
pub use gpu::Gpu;
pub use input::Input;
pub use net::Net;
pub type MmioTransport<D> = mmio::Transport<D>;
