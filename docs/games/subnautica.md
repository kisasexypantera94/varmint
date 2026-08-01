# Running Subnautica in Varmint

Subnautica is playable in Varmint using Proton 9 and a small DXVK configuration change.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 9.0
Launch options: DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```

Create `$HOME/dxvk.conf` with:

```ini
d3d11.maxFeatureLevel = 10_1
```

## 1. Select Proton 9

In Steam:

1. Open **Properties** for Subnautica.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 9.0**.

## 2. Create the DXVK configuration

Open a terminal inside Varmint and run:

```bash
printf '%s\n' 'd3d11.maxFeatureLevel = 10_1' > "$HOME/dxvk.conf"
```

This limits the Direct3D 11 feature level exposed to the game to `10_1`.

## 3. Set the launch options

In the game properties, enter the following as a single line under **Launch Options**:

```text
DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```

## 4. Start the game

Launch Subnautica normally from Steam.

No additional Wine or Vulkan configuration is required for the tested setup.

## Graphics settings

Reflections have a large performance cost in Subnautica while making relatively little visual difference.

For a substantial FPS improvement, reduce or disable **Reflections** in the graphics settings. Other settings can generally remain high, depending on the desired resolution and frame rate.

## Tested configuration

```text
Subnautica
Proton 9.0
DirectX 11 through DXVK
DXVK feature level: 10_1
Launch options: DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```
