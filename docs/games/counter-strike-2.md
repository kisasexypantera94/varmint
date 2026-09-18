# Running Counter-Strike 2 in Varmint

Counter-Strike 2 is playable in Varmint using the native Linux version.

The Windows version also works through Proton with both Venus and Neptune, but VAC does not work with Proton.

## Quick setup

Add the following to your VM's `.varmint` configuration file:

```toml
[host_environment]
MVK_CONFIG_FAST_MATH_ENABLED = "0"
```

### 1. Use the native Linux version

Do not force a Proton compatibility tool in Steam.

### 2. Start the game

Launch Counter-Strike 2 normally from Steam.

No additional launch options are required for the tested setup.

## Tested configuration

```text
Counter-Strike 2
Native Linux version
Vulkan through Venus
```

`MVK_CONFIG_FAST_MATH_ENABLED=0` is required to avoid rendering artifacts.
