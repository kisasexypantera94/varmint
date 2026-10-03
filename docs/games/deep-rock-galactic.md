# Running Deep Rock Galactic in Varmint

Deep Rock Galactic is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Deep Rock Galactic.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Enable Neptune

From a terminal inside Varmint, run:

```bash
varmint-game-graphics 548430 neptune 'FSD/Binaries/Win64/FSD-Win64-Shipping.exe'
```

Then set the following Steam launch options:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

No additional configuration is required.

### 3. Select DirectX 11

When launching the game from Steam, select the **DirectX 11** option.

Do not use the DirectX 12 version.

## Known issues

### FPS drops in some Space Rig scenes

Performance can drop sharply in the Space Rig when looking at certain scenes.

This has been investigated and is caused by a storm of occlusion queries in those scenes.

The issue appears to be specific to particular areas and viewing angles in the Space Rig. Similar performance drops were not observed during missions in testing.

## Tested configuration

```text
Deep Rock Galactic
Proton 10.0
