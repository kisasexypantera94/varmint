# Running Portal 1-2 in Varmint

Portal and Portal 2 are playable in Varmint using their native Linux builds with the Vulkan renderer.

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

No additional launch options are required.

### 3. Keep the default settings

The default in-game graphics settings work without additional configuration.

## Tested configuration

```text
Portal / Portal 2
Native Linux
Vulkan
Default graphics settings
```
