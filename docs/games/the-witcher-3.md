# Running The Witcher 3 in Varmint

The Witcher 3 is playable in Varmint using the DirectX 11 version of the game.

## Quick setup

Use the following configuration:

```text
Game version:   Classic
Proton version: Proton 9.0
Renderer:       DirectX 11
Launch options: FEX_X87REDUCEDPRECISION=0 %command%
```

### 1. Select the Classic version

In Steam:

1. Open **Properties** for The Witcher 3.
2. Open **Game Versions & Betas**.
3. Select the **Classic** version of the game.

### 2. Select Proton 9

In the game properties:

1. Open **Compatibility**.
2. Enable **Force the use of a specific Steam Play compatibility tool**.
3. Select **Proton 9.0**.

### 3. Disable reduced x87 precision

Set the following Steam launch option:

```text
FEX_X87REDUCEDPRECISION=0 %command%
```

Varmint currently enables reduced x87 precision by default for compatibility with some older games.

This breaks the 32-bit REDlauncher installer and causes it to fail with a `Path not found` error. Disabling it for The Witcher 3 allows REDlauncher to install and run normally.

### 4. Use DirectX 11

In RED launcher select the DirectX 11 version of the game.

The DirectX 12 version does not currently work in Varmint.

### 5. Configure graphics

For higher graphics settings, start from the Medium preset and enable each High feature manually. **Keep NVIDIA-specific features disabled, including HairWorks**.

**Do not use the unmodified High preset**. It enables features that are not supported by the current virtual GPU and will crash the game.

## First runs

The game may crash a few times during its first runs, particularly during combat.

In testing, it usually stabilizes after roughly two to four restarts. After that, the same areas and encounters often work normally.

When the game crashes:
1. Start it again from Steam.
2. Keep the same Proton and graphics settings.
3. Retry the same save or encounter.

Do not clear or disable the DXVK caches.

## Why the first runs may crash

The current graphics stack cannot compile some pipelines used by the game correctly.

DXVK normally stores pipeline information in its state cache. This appears to change when pipelines are compiled and allows later runs to get through scenes that crashed previously.

This behavior is not fully understood yet, but the state cache is clearly relevant: with the cache disabled, the crashes were consistently reproducible. With the normal cache enabled, the game usually stabilized after several runs.

The cache does not necessarily fix the broken pipeline itself. It appears to make the failure less likely to terminate the game.

## Known limitations

* only the DirectX 11 version currently works;
* `FEX_X87REDUCEDPRECISION` must be disabled for REDlauncher;
* NVIDIA-specific graphics features are unsupported;
* the game may require several warm-up runs before becoming stable;
* occasional graphics-pipeline crashes may still occur.

## Tested configuration

```text
The Witcher 3: Wild Hunt
Steam Classic branch
Proton 9.0
DirectX 11
FEX_X87REDUCEDPRECISION=0
DXVK caches enabled
NVIDIA HairWorks disabled
```
