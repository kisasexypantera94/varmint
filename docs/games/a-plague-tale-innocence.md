# Running A Plague Tale: Innocence in Varmint

A Plague Tale: Innocence is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for A Plague Tale: Innocence.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Enable Neptune

From a terminal inside Varmint, run:

```bash
varmint-game-graphics 752590 neptune
```

Then set the following Steam launch options:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
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
Proton 10.0
DirectX 11
Neptune
Fullscreen
```