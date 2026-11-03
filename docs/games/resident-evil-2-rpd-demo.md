# Running Resident Evil 2 "R.P.D. Demo" in Varmint

Resident Evil 2 "R.P.D. Demo" is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 1168280
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

Set the Steam launch options to:

```text
varmint-graphics neptune %command%
```

No additional setup is required.

## DirectX 12 alternative

DirectX 12 also works through D3DMetal, while DirectX 11 through DXMT remains recommended:

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

## Tested configuration

```text
Resident Evil 2 "R.P.D. Demo"
Proton Experimental
DirectX 11
Neptune
DXMT
```
