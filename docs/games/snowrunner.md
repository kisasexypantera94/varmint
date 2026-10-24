# Running SnowRunner in Varmint

SnowRunner has been reported working in Varmint using Neptune.

This configuration is based on a user report and has not been tested directly by the project.

**Steering wheels are not currently supported in Varmint.**

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: varmint-graphics neptune %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for SnowRunner.
2. Open **Compatibility**.
3. Under **Select compatibility tool**, select **Proton 10.0**.

### 2. Enable Neptune

Set the following Steam launch options:

```text
varmint-graphics neptune %command%
```

No additional configuration is required.

## Reported configuration

```text
SnowRunner
Proton 10.0
DirectX 11
Neptune
Medium settings
```
