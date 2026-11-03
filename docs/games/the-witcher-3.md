# Running The Witcher 3 in Varmint

The Witcher 3 is playable in Varmint using Neptune. The Classic version also works through Venus.

**Neptune is the recommended renderer.** Remastered uses DirectX 12 through D3DMetal. The next-gen and Classic versions use the DirectX 11 path through DXMT.

Venus remains available as an alternative, but currently only works with the Classic version.

## Important graphics settings

For both Neptune and Venus, start from the **Medium** preset and enable higher-quality options manually.

**Do not use the unmodified High preset.** It enables features that are not supported by the current virtual GPU and may crash the game.

Keep NVIDIA-specific features disabled, including **HairWorks**.

## Recommended setup: Remastered

The default Remastered branch uses DirectX 12:

```text
Steam App IDs: 292030
Profile:        remastered-dx12
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Game version:   public
Proton version: Proton Experimental
Renderer:       Neptune DX12
Host backend:    D3DMetal
Launch options: varmint-graphics neptune-dx12 %command%
```

Before starting the VM, select **D3DMetal** in the Varmint launcher. In Steam, select **Proton Experimental** and use:

```text
varmint-graphics neptune-dx12 %command%
```

## Recommended setup: next-gen and Classic

These versions use DirectX 11 through DXMT.

Use the following configuration:

```text
Profile:        next-gen-neptune
Recommendation: Recommended
Evidence:       Tested
Setup:          Assisted
Runtime:        Proton
Game version:   next-gen
Proton version: Proton 10.0
Renderer:       Neptune
Host backend:    DXMT
Launch options: FEX_X87REDUCEDPRECISION=0 varmint-graphics neptune %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for The Witcher 3.
2. Open **Compatibility**.
3. Under **Select compatibility tool**, select **Proton 10.0**.

### 2. Select the next-gen version

Open **Properties → Game Versions & Betas** and select **next-gen**.

### 3. Enable Neptune

Set the following Steam launch options:

```text
FEX_X87REDUCEDPRECISION=0 varmint-graphics neptune %command%
```

### 4. Use DirectX 11

In REDlauncher, select the **DirectX 11** version of the game.

### Optional: use the Classic version

Neptune also supports the Classic version of The Witcher 3.

```text
Profile:        classic-neptune
Recommendation: Recommended
Evidence:       Tested
Setup:          Assisted
Runtime:        Proton
Game version:   Classic
Proton version: Proton 10.0
Renderer:       Neptune
Host backend:    DXMT
Launch options: FEX_X87REDUCEDPRECISION=0 varmint-graphics neptune %command%
```

The Classic version is slightly faster, but uses the older and simpler graphics.

To switch to it:

1. Open **Properties** for The Witcher 3.
2. Open **Game Versions & Betas**.
3. Select the **Classic** version.

The same Proton, Neptune and launch-option configuration can be used.

## Alternative setup: Venus

Venus can still be used instead of Neptune, but currently only with the Classic version of The Witcher 3.

First select the **Classic** version under **Properties → Game Versions & Betas**.

Venus is the default backend and requires no additional setup.

Use the following configuration:

```text
Profile:        classic-venus
Recommendation: Alternative
Evidence:       Tested
Setup:          Assisted
Runtime:        Proton
Game version:   Classic
Proton version: Proton 10.0
Renderer:       Venus
Host backend:    MoltenVK
Launch options: FEX_X87REDUCEDPRECISION=0 %command%
```

The next-gen and Remastered versions do not currently work through Venus.

The Venus path may require a few warm-up runs before becoming stable because of DXVK pipeline/cache behavior.

Do not disable or clear the DXVK caches.

## Why reduced x87 precision is disabled

Varmint enables reduced x87 precision by default for compatibility with some older games.

This breaks the 32-bit REDlauncher installer and can cause it to fail with a `Path not found` error.

For the next-gen and Classic DirectX 11 paths, use:

```text
FEX_X87REDUCEDPRECISION=0
```

## Tested configurations

### Remastered

```text
The Witcher 3: Wild Hunt — Remastered
Public branch
Proton Experimental
DirectX 12
Neptune DX12
D3DMetal
NVIDIA HairWorks disabled
```

### Next-gen and Classic through Neptune

```text
The Witcher 3: Wild Hunt
next-gen or Classic
Proton 10.0
DirectX 11
Neptune
DXMT
FEX_X87REDUCEDPRECISION=0
NVIDIA HairWorks disabled
```

### Classic through Venus

```text
The Witcher 3: Wild Hunt
Classic branch only
next-gen and Remastered not supported through Venus
Proton 10.0
DirectX 11
Venus
MoltenVK
FEX_X87REDUCEDPRECISION=0
DXVK caches enabled
NVIDIA HairWorks disabled
```
