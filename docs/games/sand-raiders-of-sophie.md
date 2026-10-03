# Running SAND: Raiders of Sophie in Varmint

SAND: Raiders of Sophie has been reported working in Varmint using Neptune.

This configuration is based on a user report and has not been tested directly by the project.

## Quick setup

Use the following configuration:

```text
Proton version: Proton Hotfix
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### 1. Select Proton Hotfix

In Steam:

1. Open **Properties** for SAND: Raiders of Sophie.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton Hotfix**.

### 2. Enable Neptune

From a terminal inside Varmint, run:

```bash
VARMINT_PROTON_ROOT="$HOME/.local/share/Steam/steamapps/common/Proton Hotfix" \
  varmint-game-graphics 1431300 neptune
```

Then set the following Steam launch options:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

No additional configuration is required.

## Reported configuration

```text
SAND: Raiders of Sophie
Proton Hotfix
