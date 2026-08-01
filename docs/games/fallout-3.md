# Running Fallout 3 in Varmint

Fallout 3 is playable in Varmint using Proton 9.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 9.0
Launch options: FEX_X87REDUCEDPRECISION=1 %command%
```

### 1. Select Proton 9

In Steam:

1. Open **Properties** for Fallout 3.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 9.0**.

### 2. Set the launch options

In the game properties, enter the following as a single line under **Launch Options**:

```text
FEX_X87REDUCEDPRECISION=1 %command%
```

`FEX_X87REDUCEDPRECISION=1` improves performance in older games that make heavy use of x87 floating-point instructions.

### 3. Start the game

Launch Fallout 3 normally from Steam.

No additional DXVK, Wine or Vulkan configuration is required for the tested setup.

## Tested configuration

```text
Fallout 3
Proton 9.0
DirectX 9 through DXVK
Launch options: FEX_X87REDUCEDPRECISION=1 %command%
```
