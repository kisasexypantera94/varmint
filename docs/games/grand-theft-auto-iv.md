# Running Grand Theft Auto IV in Varmint

Grand Theft Auto IV is playable in Varmint using Proton 10.

The Rockstar Games Launcher can be unreliable and may occasionally hang or fail to start the game.

If that happens, restarting the VM usually helps. If the launcher still refuses to work, recreating the game's Proton prefix is the most reliable fix.

If possible, allocate 24 GB of RAM to the VM.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
RAM:            24 GB recommended
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Grand Theft Auto IV.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Start the game

Launch Grand Theft Auto IV normally from Steam.

No additional launch options are required for the tested setup.

## Known issues

### Rockstar Games Launcher

The Rockstar Games Launcher may occasionally hang or fail during startup.

If that happens, stop the game in Steam and restart the VM before trying again.

The most reliable workaround so far is to put Steam into offline mode before launching the game:

1. In Steam, open **Steam → Go Offline...**
2. Launch Grand Theft Auto IV normally.

In testing, the game started successfully on the first attempt several times in a row with Steam in offline mode.

If the launcher still refuses to start, recreate the game's Proton prefix:

```bash
cd "$HOME/.local/share/Steam/steamapps/compatdata" &&
mv 12210 "12210.bak-$(date +%Y%m%d-%H%M%S)"
```

Then launch the game again from Steam.

Proton will recreate the prefix and reinstall the Rockstar Games Launcher / Social Club components.

### Shader warmup

The game may stutter heavily or appear to hang while shaders are being compiled.

If possible, leave it running and let shader compilation finish. If it becomes completely stuck, restart the game or the VM and try again.

## Tested configuration

```text
Grand Theft Auto IV
Steam App ID 12210
Proton 10.0
24 GB RAM
```
