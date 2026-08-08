# Running Dragon’s Dogma: Dark Arisen in Varmint

Dragon’s Dogma: Dark Arisen is playable in Varmint using Proton 10.

The original WMV movies prevent the game from progressing past the title screen. Converting six movie files to AVI/MJPEG fixes the issue.

## Quick setup

Use the following configuration:

```text
Proton version: Proton 10.0
Renderer:       DirectX 9
```

### 1. Select Proton 10

In Steam:

1. Open **Properties** for Dragon’s Dogma: Dark Arisen.
2. Open **Compatibility**.
3. Enable **Force the use of a specific Steam Play compatibility tool**.
4. Select **Proton 10.0**.

### 2. Convert the movie files

Make sure the game is installed and closed.

Open a terminal inside Varmint, paste the entire script below and press Enter:

```bash
set -euo pipefail

GAME="$HOME/.local/share/Steam/steamapps/common/DDDA"
MOVIES="$GAME/nativePC/movie"

sudo apt-get update
sudo apt-get install -y ffmpeg

files=(
    "event/st100ev00.wmv"
    "event/st230ev00.wmv"
    "event/st610ev21.wmv"
    "event/st240ev05.wmv"
    "title/title_A.wmv"
    "title/title_B.wmv"
)

for rel in "${files[@]}"; do
    dst="$MOVIES/$rel"
    backup="$dst.varmint-original"
    tmp="$dst.varmint-tmp.avi"

    if [[ ! -f "$dst" ]]; then
        echo "Missing file: $dst" >&2
        exit 1
    fi

    if [[ ! -f "$backup" ]]; then
        echo "Backing up: $rel"
        cp -- "$dst" "$backup"
    else
        echo "Using existing backup: $rel"
    fi

    echo "Converting: $rel"
    rm -f -- "$tmp"

    ffmpeg \
        -hide_banner \
        -loglevel warning \
        -y \
        -i "$backup" \
        -map 0:v:0 \
        -map '0:a:0?' \
        -c:v mjpeg \
        -q:v 3 \
        -pix_fmt yuvj420p \
        -c:a pcm_s16le \
        -f avi \
        "$tmp"

    mv -f -- "$tmp" "$dst"
done

echo "Dragon's Dogma movie conversion complete."
```

The script installs FFmpeg, keeps a backup of every original movie and replaces each game file only after successful conversion.

### 3. Start the game

Launch the game normally from Steam.

After pressing a key at the title screen, the game should continue to the main menu instead of remaining on a black screen.

## Known issue

Without the movie conversion, the game reaches the **“Press any key”** screen but does not progress further.

After pressing a key, the title graphic fades out and the screen remains black while the music continues. The game process remains alive, but it never reaches the main menu.

## Steam verification and updates

Steam’s **Verify integrity of game files** feature may restore the original WMV files. A game update or reinstall may do the same.

If the black screen returns, close the game and run the conversion script again.

The script keeps the original files as `*.varmint-original` and reuses those backups as conversion sources.

## Why the workaround is needed

The affected movies use WMV3 video inside ASF containers. The current Proton media path can open them but does not successfully deliver decoded frames to the game.

AVI files with MJPEG video work through the same path. The script converts the contents while preserving the original `.wmv` filenames expected by the game.

## Verify the conversion

Run:

```bash
ffprobe \
    -v error \
    -show_entries format=format_name:stream=codec_name,codec_type \
    -of default=noprint_wrappers=1 \
    "$HOME/.local/share/Steam/steamapps/common/DDDA/nativePC/movie/title/title_B.wmv"
```

The output should include:

```text
codec_name=mjpeg
codec_type=video
format_name=avi
```

## Restore the original movies

Run:

```bash
set -euo pipefail

MOVIES="$HOME/.local/share/Steam/steamapps/common/DDDA/nativePC/movie"

find "$MOVIES" \
    -type f \
    -name '*.wmv.varmint-original' \
    -print0 |
while IFS= read -r -d '' backup; do
    original="${backup%.varmint-original}"
    echo "Restoring: $original"
    cp -f -- "$backup" "$original"
done
```

## Tested configuration

```text
Dragon’s Dogma: Dark Arisen
Steam App ID 367500
Proton 10.0
DirectX 9 through DXVK
AVI/MJPEG movie conversion applied
```
