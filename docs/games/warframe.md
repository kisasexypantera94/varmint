# Running Warframe in Varmint

Warframe is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 230410
Profile:        default
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton Experimental
Renderer:       Neptune
Host backend:    DXMT
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
