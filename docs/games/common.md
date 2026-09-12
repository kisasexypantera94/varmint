# Running games in Varmint

This page covers common setup and troubleshooting for games running in Varmint.

Game-specific Proton versions, graphics backends, launch options and workarounds are listed on the individual game pages.

## Graphics backends

Varmint currently has two main paths for Windows games:

- **Venus** - the default path. DirectX games run through DXVK or VKD3D-Proton and the guest Vulkan stack.
- **Neptune** - an experimental path for DirectX games. It is currently most useful for supported DirectX 11 titles.

Use the backend recommended on the game's documentation page.

### Switching a game to Neptune

Neptune can be configured per Steam game from a terminal inside the guest:

```bash
varmint-game-graphics <steam-appid> neptune 'relative/path/to/Game.exe'
```

For example:

```bash
varmint-game-graphics 292030 neptune 'bin/x64/witcher3.exe'
```

The executable path is only needed when the game executable is below the Steam install directory.

The command installs the required files and prints the Steam launch options to use.

To switch the game back to the normal Venus path:

```bash
varmint-game-graphics <steam-appid> venus
```

To check the current configuration:

```bash
varmint-game-graphics <steam-appid> status
```

The helper only configures the graphics backend. Game-specific options such as `FEX_X87REDUCEDPRECISION` are documented separately on each game page.

## Shader and pipeline warm-up

Both graphics paths may stutter when shaders or graphics pipelines are encountered for the first time.

This is usually more noticeable with Venus and DXVK, especially when entering a new area or seeing an effect for the first time. Later runs are generally smoother once the relevant caches have been populated.

Neptune and DXMT can also compile graphics pipelines during gameplay, but first-run stutter is generally less pronounced there.

## Choosing a Proton version

Use the Proton version listed on the game's documentation page.

Different Proton releases include different versions of Wine, DXVK, VKD3D-Proton and other compatibility components, so changing Proton can affect both compatibility and performance.

When testing another Proton version, keep the rest of the game configuration unchanged so that differences are easier to identify.

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

## Steam behavior

Steam may occasionally restart itself, spend some time updating after startup or briefly disappear while switching between client processes.

It can also take a while to finish initial setup or show a newly installed game.

When launching a game, Steam may print a warning like:

```text
You are missing the following 32-bit libraries, and Steam may not run: libc.so.6
```

This warning can be safely ignored in Varmint. Steam and games can still run normally.

On the first launch of some games, Steam may also install additional runtime components before starting the game. This is normal Steam behavior.

If Steam remains closed, start it again normally from the desktop.

## When a game hangs

A game crash or hang normally does not require restarting the VM.

Try, in order:

1. Close the game normally.
2. Use Steam's **Stop** button if it is still marked as running.
3. Restart Steam if the game has exited but Steam has not noticed.
4. Restart Varmint only if the guest desktop, Steam or the graphics device remains unusable.

A game crash does not usually require restarting the whole VM.

## Switching the graphics backend for a game

Neptune can be enabled or disabled per Steam game from a terminal inside the guest. The normal/default path is Venus.

```bash
varmint-game-graphics <steam-appid> status
varmint-game-graphics <steam-appid> venus
varmint-game-graphics <steam-appid> neptune 'relative/path/to/Game.exe'
```

The executable path is only needed when the game executable is below the Steam install root. The command installs or removes the required Neptune DLLs and prints the Steam launch options for the selected mode.

Backend switching intentionally does not add debugging or game-specific options such as `PROTON_LOG` or `FEX_X87REDUCEDPRECISION`. Keep those on the individual game page or add them alongside the printed backend options when a game needs them.
## Host overlays and notifications

macOS notifications or other host windows appearing over Varmint can cause a brief stutter while a game is running.

For more consistent performance during gameplay or recording, avoid opening host overlays and consider disabling distracting notifications temporarily.
