# Running L.A. Noire in Varmint

L.A. Noire is playable in Varmint using Proton 10.

The Rockstar Games Launcher is extremely flaky under Wine and needs a somewhat magical workaround to start more or less reliably.

If the launcher gets stuck, restarting the VM usually helps. If it still refuses to start, recreating the game's Proton prefix is the most reliable fix.

The launcher is also surprisingly memory-hungry. If possible, allocate more than 16 GB of RAM to the VM. **24 GB is sufficient.**

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       DirectX 11
RAM:            24 GB recommended
Launch options: bash -c 'DLL="$HOME/.local/share/Steam/steamapps/compatdata/110800/pfx/drive_c/Program Files/Rockstar Games/Social Club/libcef.dll"; sleep 10; [[ -f "$DLL" ]] && touch "$DLL"; sleep 3; exec "$@"' -- %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for L.A. Noire.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Add the launch option

Set the following Steam launch option:

```text
bash -c 'DLL="$HOME/.local/share/Steam/steamapps/compatdata/110800/pfx/drive_c/Program Files/Rockstar Games/Social Club/libcef.dll"; sleep 10; [[ -f "$DLL" ]] && touch "$DLL"; sleep 3; exec "$@"' -- %command%
```

The Rockstar Games Launcher sometimes gets stuck during startup and never launches the game.

This workaround waits briefly, updates the timestamp of Rockstar Social Club's `libcef.dll`, waits again, and then starts the game normally.

The command is safe to use before the first launch. If `libcef.dll` has not been installed yet, the `touch` step is simply skipped.

### 3. Start the game

Launch L.A. Noire normally from Steam.

On the first run, Rockstar Games Launcher and Social Club may need to install and initialize.

## Known issues

### Rockstar Games Launcher

The Rockstar Games Launcher is unreliable under Wine and may occasionally get stuck even with the workaround above.

If that happens, stop the game in Steam and restart the VM before trying again.

If the launcher still refuses to start, recreate the game's Proton prefix:

```bash
cd "$HOME/.local/share/Steam/steamapps/compatdata" &&
mv 110800 "110800.bak-$(date +%Y%m%d-%H%M%S)"
```

Then launch the game again from Steam.

Proton will recreate the prefix and the Rockstar Games Launcher / Social Club components will be installed and initialized again.

This is the most reliable way to recover from a completely stuck launcher.

### Memory usage

The Rockstar launcher and its CEF processes can use several gigabytes of RAM by themselves.

16 GB can be tight, especially after several launch attempts.

If your host has enough memory, allocate more than 16 GB to the VM.

```text
Recommended: 24 GB
```

24 GB has been sufficient in testing.

## Tested configuration

```text
L.A. Noire
Steam App ID 110800
Proton 10.0
DirectX 11
24 GB RAM
```