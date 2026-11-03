# Running Ghostrunner Demo in Varmint

Ghostrunner Demo is playable through Neptune's DirectX 12 mode using D3DMetal.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 1249200
Profile:        dx12
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton Experimental
Renderer:       Neptune DX12
Host backend:    D3DMetal
Launch options: varmint-graphics neptune-dx12 %command% -dx12
```

### 1. Select D3DMetal on the host

In the Varmint launcher, select **D3DMetal** before starting the VM.

### 2. Enable Neptune DX12

Set the Steam launch options to:

```text
varmint-graphics neptune-dx12 %command% -dx12
```

## DirectX 11 alternative

The previous DirectX 11 path through DXMT remains available:

```text
Profile:        dx11
Recommendation: Alternative
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton Experimental
Renderer:       Neptune
Host backend:    DXMT
Launch options: varmint-graphics neptune %command% -dx11
```

## Tested configuration

```text
Ghostrunner Demo
Proton Experimental
DirectX 12
Neptune DX12
D3DMetal
```
