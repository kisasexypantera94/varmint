# Running games in Varmint

This page covers common setup and troubleshooting for games running in Varmint.

Game-specific Proton versions, graphics backends, launch options and workarounds are listed on the individual game pages.

## Graphics backends

Varmint has two main graphics backends:

- Venus - the default backend.
- Neptune - the preferred backend for supported 64-bit DirectX 11 games.

Use the backend recommended on the game's documentation page.

### Venus

Venus is the default backend and requires no additional setup.

### Neptune

Set the game's Steam launch options to:

```text
varmint-graphics neptune %command%
```

Game-specific environment variables go before the wrapper:

```text
VARIABLE=value varmint-graphics neptune %command%
```

Game arguments go after `%command%`:

```text
varmint-graphics neptune %command% -dx11
```

To switch back to Venus, remove `varmint-graphics neptune` from the launch options and launch the game normally.

### 32-bit games

Neptune currently supports 64-bit Windows games only.

Use Venus for 32-bit games such as L.A. Noire and Deus Ex: Human Revolution.

## Choosing a Proton version

**Proton 10 is the recommended baseline for Varmint.**

It has a good balance of recent Wine fixes and an older DXVK version that is compatible with our current Vulkan feature set, so it works well as a default for both Venus and Neptune.

Many Neptune games that launch directly into a 64-bit DirectX 11 executable also work with default Proton Experimental. Games with older launchers or other legacy startup components are more likely to need a specific Proton version.

Some games may work better with a newer Proton version, while others may require an older one. Check the game's documentation page first, and use [ProtonDB](https://www.protondb.com/) for known Proton versions, launch options and game-specific fixes or patches.

Follow the individual game page when one is specified.

## Fullscreen and borderless modes

Fullscreen and borderless can behave differently depending on the game.

Borderless is often simpler for window management and focus. Fullscreen can be useful when a game only exposes lower resolutions or upscaling options there.

Varmint also exposes scaled modes derived from the Mac's native Retina resolution. For example, a 3456×2168 display has a 1728×1084 half-resolution mode. Some games do not handle these non-standard fullscreen modes correctly: the resolution may appear selectable, but the game may apply it incorrectly or not switch at all. This can lead to incorrect scaling or mismatched mouse input.

If a resolution is missing, or a game has scaling, focus or performance problems in one mode, try borderless or another available resolution.

## Shader and pipeline warm-up

Games may stutter when shaders or graphics pipelines are encountered for the first time.

This is usually more noticeable with Venus, especially when entering a new area or seeing an effect for the first time. Neptune can also compile pipelines during gameplay, but first-run stutter is generally less pronounced.

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

## Graphics settings

Start with moderate graphics settings when trying a game for the first time.

Vendor-specific features, unusual antialiasing modes and advanced effects are more likely to hit unsupported parts of the graphics stack. NVIDIA-specific options such as HairWorks should remain disabled unless the game page says otherwise.

Once the game is stable, increase settings individually instead of immediately selecting the highest preset.

## Audio recovery

If audio stops working, open a terminal inside Varmint and run:

```bash
pulseaudio -k
```

PulseAudio should restart automatically.

Some games recover immediately. Others may need to be restarted.

## When a game hangs

A game crash or hang normally does not require restarting the VM.

Try, in order:

1. Close the game normally.
2. Use Steam's **Stop** button if it is still marked as running.
3. Restart Steam if the game has exited but Steam has not noticed.
4. Restart Varmint only if the guest desktop, Steam or the graphics device remains unusable.

## Host overlays and notifications

macOS notifications or other host windows appearing over Varmint can cause a brief stutter while a game is running.

For more consistent performance during gameplay or recording, avoid opening host overlays.
