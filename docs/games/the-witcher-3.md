# Running The Witcher 3 in Varmint

The Witcher 3 is playable in Varmint using either Neptune or Venus.

**Neptune is the recommended renderer.** It supports the current next-gen version of the game and the Classic version.

Venus remains available as an alternative, but currently only works with the Classic version.

## Important graphics settings

For both Neptune and Venus, start from the **Medium** preset and enable higher-quality options manually.

**Do not use the unmodified High preset.** It enables features that are not supported by the current virtual GPU and may crash the game.

Keep NVIDIA-specific features disabled, including **HairWorks**.

## Recommended setup: Neptune

Use the following configuration:

```text
Game version:   Next-gen
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: FEX_X87REDUCEDPRECISION=0 WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for The Witcher 3.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Enable Neptune

From a terminal inside Varmint, run:

```bash
varmint-game-graphics 292030 neptune 'bin/x64/witcher3.exe'
```

Then set the following Steam launch options:

```text
FEX_X87REDUCEDPRECISION=0 WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### 3. Use DirectX 11

In REDlauncher, select the **DirectX 11** version of the game.

### Optional: use the Classic version

Neptune also supports the Classic version of The Witcher 3.

The Classic version is slightly faster, but uses the older and simpler graphics.

To switch to it:

1. Open **Properties** for The Witcher 3.
2. Open **Game Versions & Betas**.
3. Select the **Classic** version.

The same Proton, Neptune and launch-option configuration can be used.

## Alternative setup: Venus

Venus can still be used instead of Neptune, but currently only with the Classic version of The Witcher 3.

First select the **Classic** version under **Properties → Game Versions & Betas**.

Then switch the game to Venus:

```bash
varmint-game-graphics 292030 venus
```

Use the following configuration:

```text
Game version:   Classic
Proton version: Proton 10.0
Renderer:       Venus
Launch options: FEX_X87REDUCEDPRECISION=0 %command%
```

The next-gen version does not currently work through Venus.

The Venus path may require a few warm-up runs before becoming stable because of DXVK pipeline/cache behavior.

Do not disable or clear the DXVK caches.

## Why reduced x87 precision is disabled

Varmint enables reduced x87 precision by default for compatibility with some older games.

This breaks the 32-bit REDlauncher installer and can cause it to fail with a `Path not found` error.

For The Witcher 3, always use:

```text
FEX_X87REDUCEDPRECISION=0
```

## Tested configurations

### Neptune

```text
The Witcher 3: Wild Hunt
Next-gen or Classic
Proton 10.0
DirectX 11
Neptune
FEX_X87REDUCEDPRECISION=0
NVIDIA HairWorks disabled
```

### Venus

```text
The Witcher 3: Wild Hunt
Classic branch only
Proton 10.0
DirectX 11
Venus
FEX_X87REDUCEDPRECISION=0
DXVK caches enabled
NVIDIA HairWorks disabled
```