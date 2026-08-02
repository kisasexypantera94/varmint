<p align="left">
  <img src="./assets/icon.png" width="150">
</p>

# varmint
Varmint is a lightweight virtual machine for Macs, built on top of Hypervisor.framework.

The current focus is gaming and hardware-accelerated graphics. It boots a Debian arm64 guest with FEX, Steam and Proton, making it possible to run Windows and Linux games on Mac.

## Download
Download the latest release

**Varmint requires macOS 26 or later.** Download the release archive, extract it and move Varmint to the Applications folder.

## Features

* hardware-accelerated Vulkan and OpenGL
* x86-64 support through FEX
* Windows games through Proton
* audio, networking and input
* shared clipboard
* Retina and high-refresh display modes

## Games

Compatibility varies between games. DirectX 9, 10 and 11 titles currently have the best chance of working through DXVK.

See [Running games in Varmint](docs/games/common.md) for general setup and troubleshooting.

| Game                                                                                       | Status | Setup Difficulty |
| ------------------------------------------------------------------------------------------ | -----: | ----: |
| [The Witcher 3: Wild Hunt](docs/games/the-witcher-3.md)                                    |      ✅ |    🟢 |
| [The Witcher: Enhanced Edition](docs/games/the-witcher-enhanced-edition.md)                |      ✅ |    🟢 |
| [Dragon's Dogma: Dark Arisen](docs/games/dragons-dogma-dark-arisen.md)                     |      ✅ |    🟡 |
| [Fallout: New Vegas](docs/games/fallout-new-vegas.md)                                      |      ✅ |    🟢 |
| [Fallout 3](docs/games/fallout-3.md)                                                       |      ✅ |    🟢 |
| [Subnautica](docs/games/subnautica.md)                                                     |      ✅ |    🟢 |

<!-- | [Subnautica: Below Zero](docs/games/subnautica-below-zero.md)                              |      ✅ |    🟢 | -->
<!-- | [Team Fortress 2](docs/games/team-fortress-2.md)                                           |      🟡 |    ⚪ | -->
<!-- | [Portal 1-2](docs/games/portal.md)                                                         |      ❌ |    ⚪ | -->
<!-- | [Age of Empires II: Definitive Edition](docs/games/age-of-empires-2-definitive-edition.md) |      ✅ |    🟠 | -->

## Demos
|                                                       The Witcher 3: Wild Hunt                                                       |                                                       Dragon's Dogma: Dark Arisen                                                       |
| :----------------------------------------------------------------------------------------------------------------------------------: | :-------------------------------------------------------------------------------------------------------------------------------------: |
| [![The Witcher 3: Wild Hunt](https://img.youtube.com/vi/44vA4ao50Ng/maxresdefault.jpg)](https://www.youtube.com/watch?v=44vA4ao50Ng) | [![Dragon's Dogma: Dark Arisen](https://img.youtube.com/vi/MofyWR6YVBw/maxresdefault.jpg)](https://www.youtube.com/watch?v=MofyWR6YVBw) |

|                                                       Fallout: New Vegas                                                       |                                                       Subnautica                                                       |
| :----------------------------------------------------------------------------------------------------------------------------: | :--------------------------------------------------------------------------------------------------------------------: |
| [![Fallout: New Vegas](https://img.youtube.com/vi/fEr_Hq2IWoA/maxresdefault.jpg)](https://www.youtube.com/watch?v=fEr_Hq2IWoA) | [![Subnautica](https://img.youtube.com/vi/l5Vk417vImU/maxresdefault.jpg)](https://www.youtube.com/watch?v=l5Vk417vImU) |

## Graphics
Vulkan:
`guest Vulkan => Mesa Venus => virglrenderer => MoltenVK => Metal`

OpenGL:
`guest OpenGL => Mesa VirGL => virglrenderer => ANGLE => Metal`

The Vulkan path is currently the main focus.

## Building from source
Requires macOS 26 or later, Xcode, Rust and Docker.

```sh
brew install cmake meson ninja pkg-config python
make app
```

## Acknowledgements
Varmint relies on several forks maintained by the UTM project, particularly in the graphics stack.

Huge thanks to the UTM contributors for making this work possible.

## Resources
* [Booting AArch64 Linux](https://docs.kernel.org/arch/arm64/booting.html)
* [VIRTIO](https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html)
