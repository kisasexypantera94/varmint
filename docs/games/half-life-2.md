# Running Half-Life 2 in Varmint

Half-Life 2 is playable in Varmint using the native Linux build with the Vulkan renderer.

## Quick setup

Use the following configuration:

```text
Runtime:           Native Linux
Renderer:          Vulkan
Graphics settings: Default
Launch options:    -vulkan
```

### 1. Use the native Linux version

In Steam, leave **Force the use of a specific Steam Play compatibility tool** disabled.

### 2. Launch the Vulkan version

In the game properties, enter the following as a single line under **Launch Options**:

```text
-vulkan
```

Start Half-Life 2 from Steam.

No additional launch options are required.

### 3. Keep the default settings

The default in-game graphics settings work without additional configuration.

## Tested configuration

```text
Half-Life 2
Native Linux
Vulkan
Default graphics settings
```
