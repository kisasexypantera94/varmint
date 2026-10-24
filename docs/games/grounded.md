# Running Grounded in Varmint

Grounded is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       Neptune
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
```