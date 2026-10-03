# Running Grounded in Varmint

Grounded is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command% -dx11
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Grounded.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Enable Neptune

From a terminal inside Varmint, run:

```bash
varmint-game-graphics 962130 neptune 'Maine/Binaries/Win64/Maine-Win64-Shipping.exe'
```

Then set the following Steam launch options:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command% -dx11
```

No additional configuration is required.

## Known issues

### Black pixel and line artifacts

Some surfaces may show small black pixel artifacts or short horizontal black lines, especially when using the **Low** graphics preset.

The artifacts are most visible on the ground and may change as the camera moves. They are significantly less noticeable at higher graphics quality settings, but may still occasionally appear.

Using the **Medium** preset or higher is recommended if the artifacts are distracting.

This seems to be a rendering issue in the Neptune or DXMT. More research required.

## Tested configuration

```text
Grounded
Proton 10.0
DirectX 11
Neptune
```