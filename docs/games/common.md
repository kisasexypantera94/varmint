# Running games in Varmint

This page covers common setup and troubleshooting for games running in Varmint.

Game-specific Proton versions, graphics backends, launch options and workarounds are listed on the individual game pages.

## Graphics backends

Varmint currently has two main paths for Windows games:

- **Venus** - the default path. DirectX games run through DXVK or VKD3D-Proton and the guest Vulkan stack.
- **Neptune** - an experimental path for DirectX games. It is currently most useful for supported DirectX 11 titles.

Use the backend recommended on the game's documentation page.

The graphics backend can be configured per Steam game from a terminal inside the guest:

```bash
varmint-game-graphics <steam-appid> status
varmint-game-graphics <steam-appid> venus
varmint-game-graphics <steam-appid> neptune 'relative/path/to/Game.exe'
```

For example:

```bash
varmint-game-graphics 292030 neptune 'bin/x64/witcher3.exe'
```

The executable path is only needed when the game executable is below the Steam install directory.

The helper installs or removes the required Neptune files and prints the Steam launch options for the selected backend.

It only configures the graphics backend. Game-specific options such as `FEX_X87REDUCEDPRECISION`, `PROTON_LOG` or other workarounds are documented separately on each game page.

## Choosing a Proton version

**Proton 10 is the recommended baseline for Varmint.**

It has a good balance of recent Wine fixes and an older DXVK version that is compatible with our current Vulkan feature set, so it works well as a default for both Venus and Neptune.

Some games may work better with a newer Proton version, while others may require an older one. Check the game's documentation page first, and use [ProtonDB](https://www.protondb.com/) for known Proton versions, launch options and game-specific fixes or patches.

## Shader and pipeline warm-up

Both graphics paths may stutter when shaders or graphics pipelines are encountered for the first time.

This is usually more noticeable with Venus and DXVK, especially when entering a new area or seeing an effect for the first time. Later runs are generally smoother once the relevant caches have been populated.

Neptune and DXMT can also compile graphics pipelines during gameplay, but first-run stutter is generally less pronounced there.

## Steam launch options

Steam launch options must be entered as a single line.

Environment variables go before `%command%`:

```text
VARIABLE=value %command%
```

Multiple variables are separated by spaces:

```text
VARIABLE_A=value VARIABLE_B=value %command%
```

Use `$HOME` rather than `~` inside environment-variable values:

```text
DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```

Tilde expansion is not reliable in every position inside Steam launch options.

## Graphics settings

Start with moderate graphics settings when trying a game for the first time.

Vendor-specific features, unusual antialiasing modes and advanced effects are more likely to hit unsupported parts of the graphics stack. NVIDIA-specific options such as HairWorks should remain disabled unless the game page says otherwise.

Once the game is stable, increase settings individually instead of immediately selecting the highest preset.

## Audio recovery

Audio can occasionally stop working in a running game.

Open a terminal inside Varmint and run:

```bash
pulseaudio -k
```

PulseAudio should restart automatically.

Some games recover immediately. Others initialize their audio device only at startup and need to be restarted afterwards. Dragon's Dogma: Dark Arisen is one known example.

If resetting PulseAudio does not help, close and reopen the game.

## When a game hangs

A game crash or hang normally does not require restarting the VM.

Try, in order:

1. Close the game normally.
2. Use Steam's **Stop** button if it is still marked as running.
3. Restart Steam if the game has exited but Steam has not noticed.
4. Restart Varmint only if the guest desktop, Steam or the graphics device remains unusable.

## Host overlays and notifications

macOS notifications or other host windows appearing over Varmint can cause a brief stutter while a game is running.

For more consistent performance during gameplay or recording, avoid opening host overlays and consider disabling distracting notifications temporarily.
