<p align="left">
  <img src="./assets/icon.png" width="150">
</p>

# varmint
Varmint is a lightweight virtual machine for Macs, built on top of Hypervisor.framework.

The current focus is gaming and hardware-accelerated graphics. It boots a Debian arm64 guest with FEX, Steam and Proton, making it possible to run Windows and Linux games on Mac.

## How to use
1. Download the [latest release](https://github.com/kisasexypantera94/varmint/releases), extract it and move `Varmint.app` to Applications.

   **Varmint requires macOS 26 or later.**

2. Create a VM configuration:
   ```bash
   cat > "$HOME/fresh.varmint" <<'EOF'
   format_version = 1

   memory_mib = 16384
   vcpus = 8

   disk = "fresh.raw"
   disk_size_gib = 24 # can be increased later
   EOF
   ```

   The disk image will be created next to the configuration file. To expand it later, stop the VM and increase `disk_size_gib` before the next launch. Disk shrinking is not supported.

3. Open Varmint and select `fresh.varmint`.
   Since Varmint is not signed or notarized, macOS will block the first launch. Go to **System Settings → Privacy & Security**, click **Open Anyway** and confirm.

4. Wait for Debian to start, create a user and password, then log in. Initial provisioning may take a few more minutes.

5. Open **Applications → Games → Steam**, wait for Steam to finish setting up, then sign in and install a game.

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
