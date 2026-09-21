# Running Grand Theft Auto IV in Varmint

Grand Theft Auto IV: The Complete Edition works well in Varmint using Proton 10.

The Rockstar Games Launcher is currently unreliable in Varmint. The recommended workaround is to install RGLess, which removes the launcher from the game's startup path.

**Thanks to `dr_strangekebab` for pointing out the RGLess workaround.**

## Quick setup

```text
Proton version: Proton 10.0
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Grand Theft Auto IV.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Install RGLess

Install `unzip` first:

```bash
sudo apt-get update
sudo apt-get install -y unzip
```

Then install RGLess:

```bash
GAME="$HOME/.local/share/Steam/steamapps/common/Grand Theft Auto IV/GTAIV"
ZIP="/tmp/RGLessIV.zip"
TMP="/tmp/rgless-install"

cd "$GAME"

mv GTAIV.exe GTAIVbak.exe
mv PlayGTAIV.exe PlayGTAIVbak.exe
mv binkw32.dll binkw32bak.dll

curl -fL \
  "https://archive.org/download/rgless-iv/RGLessIV.zip" \
  -o "$ZIP"

rm -rf "$TMP"
mkdir -p "$TMP"

unzip -q "$ZIP" -d "$TMP"
cp -av "$TMP"/. "$GAME"/
```

RGLess replaces `GTAIV.exe`, `PlayGTAIV.exe`, and `binkw32.dll`. The original files are kept as `GTAIVbak.exe`, `PlayGTAIVbak.exe`, and `binkw32bak.dll`.

### 3. Start the game

Launch Grand Theft Auto IV normally from Steam.

No additional launch options are required. The Rockstar Games Launcher should no longer appear.

## Save files

RGLess uses a local save directory instead of the normal Rockstar Games Launcher location:

```text
~/.local/share/Steam/steamapps/common/Grand Theft Auto IV/GTAIV/save/GTA IV
```

Existing saves may need to be copied there manually.

Rockstar cloud saves are not available when using RGLess.

## Known issues

### Shader warmup

The game may stutter heavily or appear to hang while shaders are being compiled.

If possible, leave it running and let shader compilation finish. If it becomes completely stuck, restart the game or the VM and try again.

## Tested configuration

```text
Grand Theft Auto IV: The Complete Edition
Steam App ID 12210
Proton 10.0
RGLess
```
