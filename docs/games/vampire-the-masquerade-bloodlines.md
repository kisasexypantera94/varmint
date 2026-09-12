# Running Vampire: The Masquerade – Bloodlines in Varmint

Vampire: The Masquerade – Bloodlines is playable in Varmint using Proton 10 and the Unofficial Patch.

The Source engine's default audio buffering also causes noticeable audio latency. Reducing `snd_mixahead` significantly improves synchronization.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       DirectX 9
Patch:          Unofficial Patch 11.5 Basic
Launch options: -game Unofficial_Patch
snd_mixahead:   0.05
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Vampire: The Masquerade – Bloodlines.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

Launch the game once. It is expected to exit with an out-of-memory error at this point, this is normal and is the reason we need a patch.

This creates the Proton prefix required by the patch installer.

### 2. Install Firefox ESR

Varmint does not include a browser by default.

Open a terminal and run:

```bash
sudo apt-get update
sudo apt-get install -y firefox-esr
```

### 3. Download the Unofficial Patch

Open the Unofficial Patch 11.5 page:

```bash
firefox-esr 'https://www.moddb.com/mods/vtmb-unofficial-patch/downloads/bloodlines-unofficial-patch-115'
```

Download the Windows installer to your `Downloads` directory.

The default **Basic Patch** is sufficient for Varmint.

### 4. Install the patch

Make sure the game is closed.

Open a terminal, paste the entire script below and press Enter:

```bash
set -euo pipefail

STEAM="$HOME/.local/share/Steam"
GAME="$STEAM/steamapps/common/Vampire The Masquerade - Bloodlines"
COMPAT="$STEAM/steamapps/compatdata/2600"
PREFIX="$COMPAT/pfx"
PROTON="$STEAM/steamapps/common/Proton 10.0/proton"

INSTALLER="$(
    find "$HOME/Downloads" \
        -maxdepth 1 \
        -type f \
        -iname 'VTMBup*.exe' \
        -print |
    sort -V |
    tail -n 1
)"

if [[ ! -f "$GAME/vampire.exe" ]]; then
    echo "Vampire: The Masquerade - Bloodlines is not installed." >&2
    exit 1
fi

if [[ ! -d "$PREFIX" ]]; then
    echo "Proton prefix not found." >&2
    echo "Launch the game once from Steam, close it, then run this script again." >&2
    exit 1
fi

if [[ -z "$INSTALLER" || ! -f "$INSTALLER" ]]; then
    echo "Unofficial Patch installer not found in ~/Downloads." >&2
    exit 1
fi

WIN_STEAM="$PREFIX/drive_c/Program Files (x86)/Steam"
WIN_GAME="$WIN_STEAM/steamapps/common/Vampire The Masquerade - Bloodlines"

mkdir -p "$(dirname "$WIN_GAME")"

if [[ ! -e "$WIN_GAME" && ! -L "$WIN_GAME" ]]; then
    ln -s "$GAME" "$WIN_GAME"
fi

echo "Installer: $INSTALLER"

STEAM_COMPAT_CLIENT_INSTALL_PATH="$STEAM" \
STEAM_COMPAT_DATA_PATH="$COMPAT" \
"$PROTON" run "$INSTALLER"
```

The installer should detect the game automatically at:

```text
C:\Program Files (x86)\Steam\steamapps\common\Vampire The Masquerade - Bloodlines
```

Keep the default **Basic Patch** selected and finish the installation normally.

### 5. Set launch options

Open the game's **Properties** in Steam and set:

```text
-game Unofficial_Patch
```

### 6. Reduce audio latency

Open a terminal and run:

```bash
set -euo pipefail

CFG="$HOME/.local/share/Steam/steamapps/common/Vampire The Masquerade - Bloodlines/Unofficial_Patch/cfg/autoexec.cfg"

mkdir -p "$(dirname "$CFG")"

if grep -qE '^[[:space:]]*snd_mixahead' "$CFG" 2>/dev/null; then
    sed -i -E \
        's/^[[:space:]]*snd_mixahead.*/snd_mixahead "0.05"/' \
        "$CFG"
else
    printf '\nsnd_mixahead "0.05"\n' >> "$CFG"
fi

grep -n 'snd_mixahead' "$CFG"
```

This sets:

```text
snd_mixahead "0.05"
```

The default Source engine audio buffer can introduce several hundred milliseconds of delay. A value of `0.05` reduces the buffer to approximately 50 ms and significantly improves dialogue synchronization.

Lower values may cause audio underruns or crackling.

### 7. Start the game

Launch Vampire: The Masquerade – Bloodlines normally from Steam.

## Known issues

### Audio delay

Without the `snd_mixahead` adjustment, dialogue audio can noticeably lag behind character animations.

Use:

```text
snd_mixahead "0.05"
```

### Reinstalling the game or patch

Steam's **Verify integrity of game files** normally leaves the separate `Unofficial_Patch` directory intact.

However, reinstalling the game, recreating the Proton prefix, or reinstalling the patch may require repeating the relevant setup steps.

The audio setting is stored in:

```text
Unofficial_Patch/cfg/autoexec.cfg
```

and may need to be reapplied after updating or reinstalling the patch.

## Tested configuration

```text
Vampire: The Masquerade – Bloodlines
Steam App ID 2600
Proton 10.0
DirectX 9
Unofficial Patch 11.5 Basic
Launch options: -game Unofficial_Patch
snd_mixahead 0.05
```
