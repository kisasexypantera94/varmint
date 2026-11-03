# Running A Plague Tale: Innocence in Varmint

A Plague Tale: Innocence is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 752590
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

### 1. Enable Neptune

Set the following Steam launch options:

```text
varmint-graphics neptune %command%
```

No additional configuration is required.

## Black screen on startup

The game may initially open to a black screen and appear to be stuck.

If this happens, simply press **Space** or **Enter**. The game should then continue loading normally.

## Enable fullscreen

The game may start in windowed mode by default.

Once in the game, open the graphics settings and switch the display mode to **Fullscreen**.

Fullscreen also provides better performance in the tested setup.

## Tested configuration

```text
A Plague Tale: Innocence
Proton Experimental
DirectX 11
Neptune
Fullscreen
```
