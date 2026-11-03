# Running Skyrim Special Edition in Varmint

The Elder Scrolls V: Skyrim Special Edition is playable in Varmint using Neptune.

## Quick setup

Use the following configuration:

```text
Steam App IDs: 489830
Profile:        default
Recommendation: Recommended
Evidence:       Tested
Setup:          Automatic
Runtime:        Proton
Proton version: Proton 10.0
Renderer:       Neptune
Host backend:    DXMT
Launch options: varmint-graphics neptune %command%
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Skyrim Special Edition.
2. Open **Compatibility**.
3. Under **Select compatibility tool**, select **Proton 10.0**.

### 2. Enable Neptune

Set the Steam launch options to:

```text
varmint-graphics neptune %command%
```

The Skyrim launcher can be used normally to configure graphics settings before starting the game.

No additional setup is required.

## Tested configuration

```text
The Elder Scrolls V: Skyrim Special Edition
Proton 10.0
DirectX 11
Neptune
```

## Known issues

### Fullscreen resolutions missing in the launcher

On displays with non-standard aspect ratios, the Skyrim launcher may only offer the native resolution in fullscreen mode.

To force 1920×1200 fullscreen, run this in the VM terminal:

```bash
PREFS="$HOME/.local/share/Steam/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/Documents/My Games/Skyrim Special Edition/SkyrimPrefs.ini"

cp "$PREFS" "$PREFS.bak"

sed -i \
  -e 's/^bFull Screen=.*/bFull Screen=1/' \
  -e 's/^bBorderless=.*/bBorderless=0/' \
  -e 's/^iSize W=.*/iSize W=1920/' \
  -e 's/^iSize H=.*/iSize H=1200/' \
  "$PREFS"
```

Launch Skyrim without reopening the launcher's graphics options, as they may overwrite these settings.
