<p align="left">
  <img src="./assets/icon.png" width="100">
</p>

# Varmint
Varmint is a lightweight virtual machine for Macs, built on top of Hypervisor.framework.

The current focus is gaming. It runs Linux with hardware-accelerated graphics, making it possible to play Windows and Linux games on Apple Silicon Macs.

Varmint provides Venus for Vulkan and Neptune for DirectX 11 and 12 games.

## How to use
1. Download the [latest release](https://github.com/kisasexypantera94/varmint/releases), extract it and move `Varmint.app` to Applications.

   **Varmint requires macOS 26 or later.**

2. Open Varmint.
   Since Varmint is not signed or notarized, macOS will block the first launch. Go to System Settings → Privacy & Security, click Open Anyway and confirm.

3. Create a VM and launch it.

4. Wait for Debian to start, create a user and password, then log in.

5. Open Steam, wait for it to finish setting up, then sign in and install a game.

See [Using Varmint](docs/using-varmint.md) for keyboard shortcuts, clipboard sharing and other VM basics.

## Controllers
Game controllers supported by macOS work automatically, even when connected after the VM starts. They appear as Xbox 360 controllers, with rumble where available.

Turn on **DualSense passthrough** in the launcher settings to use a DualSense with its extra features. It needs Input Monitoring access in System Settings → Privacy & Security, otherwise, the controller falls back to Xbox 360 mode.

## Games

Compatibility varies between games and graphics paths:
- Use Venus for DX9 and 32-bit DX11 games.
- For 64-bit DX11 games, try Neptune first and use Venus as a fallback.
- For 64-bit DX12 games, use Neptune's DX12 mode with D3DMetal. D3DMetal requires macOS 26.4 or later, and DX12 has been tested on fewer games than DX11.
- Proton 10 is the recommended baseline. Many Neptune games also work with Proton Experimental.
- Neptune also works with Proton's ARM64 builds, which can help games that crash at startup on regular Proton.

See [Running games in Varmint](docs/games/common.md) for general setup and troubleshooting.

The games below have been tested. Click a game name for its tested configuration and game-specific setup notes.

| Game                                                                                    | Status | Setup Difficulty |
| --------------------------------------------------------------------------------------- | -----: | ---------------: |
| [The Witcher 3: Wild Hunt](docs/games/the-witcher-3.md)                                 |      ✅ |               🟢 |
| [The Witcher: Enhanced Edition](docs/games/the-witcher-enhanced-edition.md)             |      ✅ |               🟢 |
| [Dragon's Dogma: Dark Arisen](docs/games/dragons-dogma-dark-arisen.md)                  |      ✅ |               🟡 |
| [Fallout: New Vegas](docs/games/fallout-new-vegas.md)                                   |      ✅ |               🟢 |
| [Fallout 3](docs/games/fallout-3.md)                                                    |      ✅ |               🟢 |
| [Subnautica](docs/games/subnautica.md)                                                  |      ✅ |               🟢 |
| [Portal 1-2](docs/games/portal.md)                                                      |      ✅ |               🟢 |
| [Team Fortress 2](docs/games/team-fortress-2.md)                                        |      ✅ |               🟢 |
| [Half-Life 2](docs/games/half-life-2.md)                                                |      ✅ |               🟢 |
| [Dishonored](docs/games/dishonored.md)                                                  |      ✅ |               🟢 |
| [Deus Ex: Human Revolution](docs/games/deus-ex-human-revolution.md)                     |      ✅ |               🟢 |
| [SCP – Containment Breach](docs/games/scp-containment-breach.md)                        |      ✅ |               🟢 |
| [L.A. Noire](docs/games/la-noire.md)                                                    |      ✅ |               🟠 |
| [Vampire: The Masquerade - Bloodlines](docs/games/vampire-the-masquerade-bloodlines.md) |      ✅ |               🟡 |
| [A Plague Tale: Innocence](docs/games/a-plague-tale-innocence.md)                       |      ✅ |               🟢 |
| [Garry's Mod](docs/games/garrys-mod.md)                                                 |      ✅ |               🟢 |
| [Red Comrades 2: For the Great Justice. Reloaded](docs/games/red-comrades-reloaded.md)  |      ✅ |               🟢 |
| [Red Comrades Save the Galaxy: Reloaded](docs/games/red-comrades-reloaded.md)           |      ✅ |               🟢 |
| [Counter-Strike 2](docs/games/counter-strike-2.md)                                      |      ✅ |               🟡 |
| [Stoneshard Demo](docs/games/stoneshard.md)                                             |      ✅ |               🟢 |
| [Assassin's Creed II](docs/games/assassins-creed-2.md)                                  |      ✅ |               🟢 |
| [SnowRunner](docs/games/snowrunner.md)                                                  |      ✅ |               🟢 |
| [Grand Theft Auto IV](docs/games/grand-theft-auto-iv.md)                                |      ✅ |               🟡 |
| [SAND: Raiders of Sophie](docs/games/sand-raiders-of-sophie.md)                         |      ✅ |               🟢 |
| [Muck](docs/games/muck.md)                                                              |      ✅ |               🟢 |
| [Burglin' Gnomes](docs/games/burglin-gnomes.md)                                         |      ✅ |               🟢 |
| [Grounded](docs/games/grounded.md)                                                      |      ✅ |               🟢 |
| [Deep Rock Galactic](docs/games/deep-rock-galactic.md)                                  |      ✅ |               🟢 |
| [DARK SOULS II: Scholar of the First Sin](docs/games/dark-souls-2-sotfs.md)             |      ✅ |               🟢 |
| [Assassin's Creed Odyssey](docs/games/assassins-creed-odyssey.md)                       |      ✅ |               🟢 |
| [The Elder Scrolls V: Skyrim Special Edition](docs/games/skyrim-special-edition.md)     |      ✅ |               🟢 |
| [Dark Deception](docs/games/dark-deception.md)                                          |      ✅ |               🟢 |
| [Resident Evil 2 "R.P.D. Demo"](docs/games/resident-evil-2-rpd-demo.md)                 |      ✅ |               🟢 |
| [Resident Evil 3: Raccoon City Demo](docs/games/resident-evil-3-raccoon-city-demo.md)   |      ✅ |               🟢 |
| [FINAL FANTASY XV WINDOWS EDITION PLAYABLE DEMO](docs/games/final-fantasy-xv-demo.md)   |      ✅ |               🟢 |
| [The Dark Pictures Anthology: The Devil in Me Demo](docs/games/the-dark-pictures-the-devil-in-me-demo.md) |      ✅ |               🟢 |
| [Ghostrunner Demo](docs/games/ghostrunner-demo.md)                                      |      ✅ |               🟢 |
| [Guild Wars 2](docs/games/guild-wars-2.md)                                              |      ✅ |               🟢 |
| [Risk of Rain 2](docs/games/risk-of-rain-2.md)                                          |      ✅ |               🟢 |
| [Batman: Arkham Asylum](docs/games/batman-arkham-asylum.md)                             |      ✅ |               🟢 |
| [Warframe](docs/games/warframe.md)                                                      |      ✅ |               🟢 |
| [Stellar Blade Demo](docs/games/stellar-blade-demo.md)                                  |      ✅ |               🟢 |
| [Black Myth: Wukong Benchmark Tool](docs/games/black-myth-wukong-benchmark.md)          |      ✅ |               🟢 |
| [BioShock Remastered](docs/games/bioshock-remastered.md)                                |      ✅ |               🟢 |

## Demos
|                                                       The Witcher 3: Wild Hunt                                                       |                                                       Dragon's Dogma: Dark Arisen                                                       |
| :----------------------------------------------------------------------------------------------------------------------------------: | :-------------------------------------------------------------------------------------------------------------------------------------: |
| [![The Witcher 3: Wild Hunt](https://img.youtube.com/vi/44vA4ao50Ng/maxresdefault.jpg)](https://www.youtube.com/watch?v=44vA4ao50Ng) | [![Dragon's Dogma: Dark Arisen](https://img.youtube.com/vi/MofyWR6YVBw/maxresdefault.jpg)](https://www.youtube.com/watch?v=MofyWR6YVBw) |

|                                                       Fallout: New Vegas                                                       |                                                       Subnautica                                                       |
| :----------------------------------------------------------------------------------------------------------------------------: | :--------------------------------------------------------------------------------------------------------------------: |
| [![Fallout: New Vegas](https://img.youtube.com/vi/fEr_Hq2IWoA/maxresdefault.jpg)](https://www.youtube.com/watch?v=fEr_Hq2IWoA) | [![Subnautica](https://img.youtube.com/vi/l5Vk417vImU/maxresdefault.jpg)](https://www.youtube.com/watch?v=l5Vk417vImU) |

## Graphics

DirectX through Neptune:

```text
guest DirectX => Mesa Neptune => virglrenderer => DXMT/D3DMetal => Metal
```

Vulkan:

```text
guest Vulkan => Mesa Venus => virglrenderer => MoltenVK => Metal
```

OpenGL:

```text
guest OpenGL => Mesa VirGL => virglrenderer => ANGLE => Metal
```

Neptune is the preferred path for supported 64-bit DirectX 11 and 12 games. Venus remains the general-purpose Vulkan path and is used by DXVK and other Vulkan applications.

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
- [Booting AArch64 Linux](https://docs.kernel.org/arch/arm64/booting.html)
- [VIRTIO](https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html)
