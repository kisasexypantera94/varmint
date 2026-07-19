<p align="left">
  <img src="./assets/icon.png" width="150">
</p>

# varmint
A small Virtual Machine Manager for Apple Silicon, built on top of Hypervisor.framework.

It focuses on making Linux guests usable for real desktop workloads, including accelerated Vulkan and OpenGL.

It boots a Debian arm64 guest and a small set of devices:
- virtio-blk
- virtio-net
- virtio-input
- virtio-snd
- virtio-gpu
- virtio-console (shared clipboard)

## Graphics
The main experiment is hardware-accelerated graphics.

Supported paths:
- Vulkan: `guest Vulkan => Mesa Venus => virglrenderer => MoltenVK => Metal`
- OpenGL: `guest OpenGL => Mesa VirGL => virglrenderer => ANGLE => Metal`

x86-64 binaries can run under FEX. For example, Subnautica is playable under FEX + Proton and reaches around 60 FPS on high settings.

## Demo
Subnautica:
<p align="left">
  <img src="./assets/subnautica_demo.png">
</p>

## Setup
Requires macOS 26 or later, Xcode and Rust.

```sh
brew install cmake meson ninja pkg-config dtc python
make app KERNEL=/path/to/Image INITRD=/path/to/initrd
```

## Resources
- [Booting AArch64 Linux](https://docs.kernel.org/arch/arm64/booting.html)
- [VIRTIO](https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html)
