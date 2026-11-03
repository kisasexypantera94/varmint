# Running Deep Rock Galactic in Varmint

Deep Rock Galactic is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 548430
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

### 2. Select DirectX 11

When launching the game from Steam, select the **DirectX 11** option.

DirectX 11 through DXMT remains the recommended path.

## DirectX 12 alternative

DirectX 12 also works through D3DMetal:

```text
Profile:        dx12
Recommendation: Alternative
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton Experimental
Renderer:       Neptune DX12
Host backend:    D3DMetal
Launch options: varmint-graphics neptune-dx12 %command% -dx12
```

Select **D3DMetal** in the Varmint launcher before starting the VM, then use the launch options above.

## Known issues

### FPS drops in some Space Rig scenes

Performance can drop sharply in the Space Rig when looking at certain scenes.

This has been investigated and is caused by a storm of occlusion queries in those scenes.

The issue appears to be specific to particular areas and viewing angles in the Space Rig. Similar performance drops were not observed during missions in testing.

## Tested configuration

```text
Deep Rock Galactic
Proton Experimental
DirectX 11
Neptune
DXMT
```
