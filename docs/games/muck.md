# Running Muck in Varmint

Muck is playable in Varmint using Neptune.

## Quick setup

The recommended configuration is:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

## 1. Select Proton 10

In Steam:

1. Open **Properties** for Muck.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

## 2. Enable Neptune

Open a terminal inside Varmint and run:

```bash
varmint-game-graphics 1625450 neptune
```

## 3. Set the launch options

In the game properties, enter the following as a single line under **Launch Options**:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

## 4. Start the game

Launch Muck normally from Steam.

No additional Wine or graphics configuration is required.

## Tested configuration

```text
Muck
Proton 10.0
