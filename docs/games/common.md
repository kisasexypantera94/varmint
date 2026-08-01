# Running games in Varmint

This page covers common behavior and troubleshooting that applies to multiple games.

Game-specific Proton versions, launch options and workarounds are listed on the individual game pages.

## DirectX 9 games

As a rule of thumb, use the following launch option for older DirectX 9 games:

```text
FEX_X87REDUCEDPRECISION=1 %command%
```

Many older games make heavy use of x87 floating-point instructions. Reduced x87 precision can significantly improve their performance under FEX.

This option is part of the tested configuration for games such as:

* Fallout 3
* Fallout: New Vegas
* The Witcher: Enhanced Edition
* Dragon’s Dogma: Dark Arisen

It is not required for every DirectX 9 game, so prefer the configuration listed on the game’s documentation page when one is available.

## Shader and pipeline warm-up

Some games may stutter or occasionally crash during their first few runs as new shaders and graphics pipelines are encountered.

Later runs are often smoother because DXVK and the graphics stack can reuse cached pipeline information.

For normal gameplay:

* leave the DXVK state and shader caches enabled;
* retry the game before changing its configuration after a one-off pipeline crash;
* expect new areas, effects or combat encounters to stutter more on their first appearance.

Do not normally use:

```text
DXVK_STATE_CACHE=0
DXVK_SHADER_CACHE=0
```

Disabling the caches can make both stuttering and graphics-pipeline failures substantially more reproducible.

## Audio recovery

Audio can occasionally stop working in a running game.

First, open a terminal inside Varmint and run:

```bash
pulseaudio -k
```

PulseAudio should restart automatically.

Some games recover immediately. Others initialize their audio device only during startup and must be restarted after PulseAudio is reset. Dragon’s Dogma: Dark Arisen is one known example.

If restarting PulseAudio does not help, close and reopen the game.

## Steam behavior

Steam may occasionally:

* close and reopen itself;
* spend some time updating after startup;
* briefly disappear while switching between client processes;
* take longer than expected to show an installed game or complete its initial setup.

This is generally normal. Give Steam time to finish updating before restarting Varmint or reinstalling anything.

If Steam remains closed, start it again from the desktop normally.

## First game launch

The first launch of a game can take noticeably longer than later launches.

Steam and Proton may need to:

* create the game’s Proton prefix;
* install runtime components;
* process shader-cache data;
* initialize DXVK caches.

A blank window or a long pause during the first launch does not always mean the game has failed. Later launches are usually faster.

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

## Choosing a Proton version

Newer Proton versions are not always better for the current Varmint graphics stack.

Use the Proton version listed on the game’s documentation page. Changing Proton can also change the bundled versions of DXVK, WineVulkan and other compatibility components.

When testing a different Proton version, keep the rest of the configuration unchanged so that any difference is easier to identify.

## Graphics settings

Start with moderate graphics settings when testing a game for the first time.

Vendor-specific features, unusual antialiasing modes and advanced effects are more likely to expose unsupported graphics paths. In particular, NVIDIA-specific options such as HairWorks should remain disabled unless they have been tested.

Once the game is stable, increase settings individually rather than selecting the highest preset immediately.

## Host overlays and notifications

macOS notifications or other windows appearing over Varmint can cause a brief stutter while a game is running.

For consistent performance during gameplay or recording, avoid opening host overlays and consider disabling distracting notifications temporarily.

## When a game hangs

Before restarting the entire VM:

1. Try closing the game through Steam.
2. Use Steam’s **Stop** button if the game is still marked as running.
3. Restart Steam if the game process has exited but Steam has not noticed.
4. Restart Varmint only if the guest desktop, Steam or the graphics device remains unusable.

A game crash does not usually require restarting the whole VM.
