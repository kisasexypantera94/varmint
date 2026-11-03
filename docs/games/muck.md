# Running Muck in Varmint

Muck is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 1625450
Profile:        default
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton 10.0
Renderer:       Neptune
Host backend:    DXMT
Launch options: varmint-graphics neptune %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Muck.
2. Open **Compatibility**.
3. Under **Select compatibility tool**, select **Proton 10.0**.

### 2. Enable Neptune

Set the Steam launch options to:

```text
varmint-graphics neptune %command%
```

No additional setup is required.

## Tested configuration

```text
Muck
Proton 10.0
DirectX 11
Neptune
```
