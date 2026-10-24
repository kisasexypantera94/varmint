# Running Subnautica in Varmint

Subnautica is playable in Varmint using either Neptune or Venus.

Neptune is recommended. Venus also works, but requires limiting the DirectX 11 feature level.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       Neptune
Launch options: varmint-graphics neptune %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Subnautica.
2. Open **Compatibility**.
3. Under **Select compatibility tool**, select **Proton 10.0**.

### 2. Enable Neptune

Set the Steam launch options to:

```text
varmint-graphics neptune %command%
```

No additional setup is required.

## Graphics settings

Reflections have a large performance cost in Subnautica while making relatively little visual difference.

For a substantial FPS improvement, reduce or disable **Reflections**. Other settings can generally remain high depending on the desired resolution and frame rate.

## Using Venus instead

Venus also works, but requires limiting the DirectX 11 feature level exposed by DXVK.

Create `$HOME/dxvk.conf` with:

```bash
printf '%s\n' 'd3d11.maxFeatureLevel = 10_1' > "$HOME/dxvk.conf"
```

Then use:

```text
DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```

## Tested configurations

### Neptune

```text
Subnautica
Proton 10.0
DirectX 11
Neptune
```

### Venus

```text
Subnautica
Proton 10.0
DirectX 11
Venus
DXVK feature level: 10_1
Launch options: DXVK_CONFIG_FILE="$HOME/dxvk.conf" %command%
```
