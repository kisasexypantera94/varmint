# Running Warframe in Varmint

Warframe is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Proton version: Proton Experimental
Renderer:       Neptune
Launch options: varmint-graphics neptune %command%
```

Steam's default Proton Experimental works, so no compatibility override is required.

### Enable Neptune

Set the Steam launch options to:

```text
varmint-graphics neptune %command%
```

## Fullscreen and borderless

Borderless is the simpler option for window management and focus.

Fullscreen is useful when you want to select a lower output resolution and use the game's upscaling options.

If fullscreen starts with incorrect scaling or keyboard focus, use the Plasma taskbar to **Minimize** Warframe once.

## Tested configuration

```text
Warframe
Proton Experimental
DirectX 11
Neptune
```
