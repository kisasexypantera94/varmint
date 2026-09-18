# Running SnowRunner in Varmint

SnowRunner has been reported working in Varmint using Neptune.

This configuration is based on a user report and has not been tested directly by the project.

**Steering wheels are not currently supported in Varmint.**

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for SnowRunner.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Enable Neptune

From a terminal inside Varmint, run:

```bash
varmint-game-graphics 1465360 neptune
```

Then set the following Steam launch options:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
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
