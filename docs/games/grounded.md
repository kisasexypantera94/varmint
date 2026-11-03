# Running Grounded in Varmint

Grounded is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 962130
Profile:        default
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton 10.0
Renderer:       Neptune
Host backend:    DXMT
Launch options: varmint-graphics neptune %command% -dx11
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Grounded.
2. Open **Compatibility**.
3. Under **Select compatibility tool**, select **Proton 10.0**.

### 2. Enable Neptune

Set the following Steam launch options:

```text
varmint-graphics neptune %command% -dx11
```

No additional configuration is required.

## DirectX 12 alternative

DirectX 12 also works through D3DMetal, while DirectX 11 through DXMT remains recommended:

```text
Profile:        dx12
Recommendation: Alternative
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton 10.0
Renderer:       Neptune DX12
Host backend:    D3DMetal
Launch options: varmint-graphics neptune-dx12 %command% -dx12
```

Select **D3DMetal** in the Varmint launcher before starting the VM, then use the launch options above.

## Known issues

### Black pixel and line artifacts

Some surfaces may show small black pixel artifacts or short horizontal black lines, especially when using the **Low** graphics preset.

The artifacts are most visible on the ground and may change as the camera moves. They are significantly less noticeable at higher graphics quality settings, but may still occasionally appear.

Using the **Medium** preset or higher is recommended if the artifacts are distracting.

This appears to be a rendering issue in the Neptune/DXMT path.

## Tested configuration

```text
Grounded
Proton 10.0
DirectX 11
Neptune
DXMT
```
