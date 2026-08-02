#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="$ROOT/build/guest"
CACHE_DIR="$ROOT/build/cache/guest"
BUILDER_IMAGE="varmint-guest-builder:trixie-arm64"
DEBIAN_BASE_URL="${DEBIAN_BASE_URL:-https://cloud.debian.org/images/cloud/trixie/latest}"
DEBIAN_IMAGE_NAME="${DEBIAN_IMAGE_NAME:-debian-13-nocloud-arm64.qcow2}"
SOURCE_IMAGE="$CACHE_DIR/$DEBIAN_IMAGE_NAME"
STAMP="$OUTPUT_DIR/.build-stamp"

INPUTS=("$SCRIPT_DIR")
OUTPUTS=(
  "$OUTPUT_DIR/Image"
  "$OUTPUT_DIR/initrd"
  "$OUTPUT_DIR/varmint-debian.raw.zst"
  "$OUTPUT_DIR/SHA256SUMS"
  "$OUTPUT_DIR/SOURCE"
)

command -v shasum >/dev/null 2>&1 || { echo "error: shasum is required" >&2; exit 1; }

fingerprint="$({
  printf '%s\n' "$DEBIAN_BASE_URL" "$DEBIAN_IMAGE_NAME"
  find "${INPUTS[@]}" -type f | LC_ALL=C sort | while IFS= read -r input; do
    printf '%s\0' "${input#$SCRIPT_DIR/}"
    cat "$input"
    printf '\0'
  done
} | shasum -a 256 | awk '{print $1}')"

outputs_ready=1
for output in "${OUTPUTS[@]}"; do
  if [ ! -s "$output" ]; then
    outputs_ready=0
    break
  fi
done

if [ "$outputs_ready" -eq 1 ] && [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$fingerprint" ]; then
  printf '\n== guest runtime is up to date ==\n'
  exit 0
fi

command -v docker >/dev/null 2>&1 || { echo "error: Docker is required to build the guest runtime" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo "error: curl is required" >&2; exit 1; }
docker info >/dev/null 2>&1 || { echo "error: Docker is not running" >&2; exit 1; }

mkdir -p "$CACHE_DIR" "$OUTPUT_DIR"
sums="$CACHE_DIR/SHA512SUMS"
sums_partial="$sums.partial.$$"
image_partial="$SOURCE_IMAGE.partial.$$"
trap 'rm -f "$sums_partial" "$image_partial"' EXIT

printf '\n== fetch Debian image checksum ==\n'
curl --fail --location --retry 3 --retry-delay 2 \
  --output "$sums_partial" \
  "$DEBIAN_BASE_URL/SHA512SUMS"
mv "$sums_partial" "$sums"

expected_sha512="$(awk -v name="$DEBIAN_IMAGE_NAME" '
  {
    file = $2
    sub(/^\*/, "", file)
    sub(/^\.\//, "", file)
    if (file == name) {
      print $1
      exit
    }
  }
' "$sums")"
[ -n "$expected_sha512" ] || { echo "error: $DEBIAN_IMAGE_NAME is missing from $DEBIAN_BASE_URL/SHA512SUMS" >&2; exit 1; }

actual_sha512=""
if [ -f "$SOURCE_IMAGE" ]; then
  actual_sha512="$(shasum -a 512 "$SOURCE_IMAGE" | awk '{print $1}')"
fi

if [ "$actual_sha512" != "$expected_sha512" ]; then
  printf '\n== download Debian nocloud arm64 image ==\n'
  rm -f "$SOURCE_IMAGE"
  curl --fail --location --retry 3 --retry-delay 2 \
    --output "$image_partial" \
    "$DEBIAN_BASE_URL/$DEBIAN_IMAGE_NAME"

  actual_sha512="$(shasum -a 512 "$image_partial" | awk '{print $1}')"
  [ "$actual_sha512" = "$expected_sha512" ] || {
    echo "error: SHA512 mismatch for $DEBIAN_IMAGE_NAME" >&2
    exit 1
  }
  mv "$image_partial" "$SOURCE_IMAGE"
else
  printf '\n== use cached Debian image ==\n'
fi

printf '\n== build guest image builder ==\n'
docker build \
  --platform linux/arm64 \
  --tag "$BUILDER_IMAGE" \
  --file "$SCRIPT_DIR/Dockerfile" \
  "$SCRIPT_DIR"

printf '\n== build guest runtime ==\n'
docker run --rm \
  --platform linux/arm64 \
  --privileged \
  --volume "$CACHE_DIR:/input:ro" \
  --volume "$OUTPUT_DIR:/output" \
  --env "HOST_UID=$(id -u)" \
  --env "HOST_GID=$(id -g)" \
  --env "SOURCE_URL=$DEBIAN_BASE_URL/$DEBIAN_IMAGE_NAME" \
  --env "SOURCE_SHA512=$expected_sha512" \
  "$BUILDER_IMAGE" "/input/$DEBIAN_IMAGE_NAME"

printf '%s\n' "$fingerprint" > "$STAMP"
printf '\nGuest runtime ready in %s\n' "$OUTPUT_DIR"
