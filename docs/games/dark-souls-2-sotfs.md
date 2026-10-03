# Running DARK SOULS II: Scholar of the First Sin in Varmint

DARK SOULS II: Scholar of the First Sin is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 11.0
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### 1. Select Proton Hotfix

In Steam:

1. Open **Properties** for DARK SOULS II: Scholar of the First Sin.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton Hotfix**.

### 2. Enable Neptune

From a terminal inside Varmint, run:

```bash
VARMINT_PROTON_ROOT="$HOME/.local/share/Steam/steamapps/common/Proton 11.0" \
  varmint-game-graphics 335300 neptune 'Game/DarkSoulsII.exe'
```

Then set the following Steam launch options:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

No additional configuration is required.

### 3. Switch Steam to Offline Mode

Before starting the game, switch Steam to **Offline Mode**.

When Steam is online, the game runs significantly slower, typically around 20-30 FPS in the tested setup.

With Steam in Offline Mode, normal performance is restored.

The same behavior has also been observed when running the game through CrossOver.

The exact cause is currently unknown.

If you find a better workaround or a fix, please let me know.

## Tested configuration

```text
DARK SOULS II: Scholar of the First Sin
Proton 11.0
