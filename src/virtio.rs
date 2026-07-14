//! https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html

mod blk;
mod chain;
mod common;
pub mod console;
mod device;
pub mod gpu;
pub mod input;
mod mmio;
mod net;
pub mod snd;
pub mod virgl_ffi;
mod virtq;

pub use blk::Blk;
pub use console::Console;
pub use device::Device;
pub use gpu::Gpu;
pub use input::Input;
pub use mmio::QUEUE_NOTIFY_OFFSET;
pub use net::Net;
pub use snd::Snd;
pub type MmioTransport<D> = mmio::Transport<D>;
