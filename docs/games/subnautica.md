# Running Subnautica in Varmint

Subnautica is playable in Varmint using either Neptune or Venus.

Neptune is the recommended graphics path. Venus also works, but requires limiting the Direct3D 11 feature level exposed by DXVK.

## Quick setup

The recommended configuration is:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

## 1. Select Proton 10

In Steam:

1. Open **Properties** for Subnautica.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

## 2. Enable Neptune

Open a terminal inside Varmint and run:

```bash
varmint-game-graphics 264710 neptune
```

## 3. Set the launch options

In the game properties, enter the following as a single line under **Launch Options**:

```text
WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

## 4. Start the game

Launch Subnautica normally from Steam.

No additional Wine or graphics configuration is required for the Neptune setup.

## Graphics settings

Reflections have a large performance cost in Subnautica while making relatively little visual difference.

For a substantial FPS improvement, reduce or disable **Reflections** in the graphics settings. Other settings can generally remain high, depending on the desired resolution and frame rate.

## Using Venus instead

Subnautica can also run through DXVK and Venus.

Switch the game back to Venus:

```bash
varmint-game-graphics 264710 venus
```

Create `$HOME/dxvk.conf` with:

```bash
printf '%s\n' 'd3d11.maxFeatureLevel = 10_1' > "$HOME/dxvk.conf"
```

Then use:

```text
DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```

This limits the Direct3D 11 feature level exposed by DXVK to `10_1`.

## Tested configurations

### Neptune

```text
Subnautica
Proton 10.0
DirectX 11 through Neptune
Launch options: WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%
```

### Venus

```text
Subnautica
Proton 10.0
DirectX 11 through DXVK and Venus
DXVK feature level: 10_1
Launch options: DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```