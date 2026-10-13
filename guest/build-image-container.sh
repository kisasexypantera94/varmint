#!/usr/bin/env bash
set -euo pipefail

SOURCE_IMAGE="${1:-}"
OUTPUT_DIR=/output
SCRIPT_DIR=/opt/varmint
PUBLISH_DIR=""

[ -n "$SOURCE_IMAGE" ] || { echo "error: source image path is required" >&2; exit 1; }
[ -f "$SOURCE_IMAGE" ] || { echo "error: missing source image: $SOURCE_IMAGE" >&2; exit 1; }

work_dir="$(mktemp -d /tmp/varmint-guest.XXXXXX)"
trap 'rm -rf "$work_dir"; [ -z "$PUBLISH_DIR" ] || rm -rf "$PUBLISH_DIR"' EXIT
work_image="$work_dir/varmint-debian.raw"
stage="$work_dir/stage"
mkdir -p "$OUTPUT_DIR" "$stage"

# The Debian nocloud image is only 3 GiB. Varmint now bakes the FEX rootfs
# and ARM64 Steam bootstrap into the image, so expand the build-time root
# filesystem before provisioning. Free zero-filled space compresses very well
# in the published .raw.zst.
truncate -s 8G "$work_image"
virt-resize     --expand /dev/sda1     "$SOURCE_IMAGE"     "$work_image"

virt-customize --network -a "$work_image" \
    --copy-in "$SCRIPT_DIR/rootfs:/tmp" \
    --run-command 'cp -a /tmp/rootfs/. / && rm -rf /tmp/rootfs' \
    --run-command '/usr/local/libexec/varmint-provision' \
    --run-command 'missing=""; for command in curl e2fsck fsck growpart resize2fs sgdisk sudo glxinfo pactl pulseaudio python3 update-initramfs vulkaninfo xclip; do command -v "$command" >/dev/null 2>&1 || missing="$missing $command"; done; [ -z "$missing" ] || { echo "missing required guest commands:$missing" >&2; exit 1; }' \
    --run-command 'for initrd in /boot/initrd.img-*; do listing="$(lsinitramfs "$initrd")"; printf "%s\n" "$listing" | grep -Eq "(^|/)sbin/fsck$" && printf "%s\n" "$listing" | grep -Eq "(^|/)sbin/e2fsck$" && printf "%s\n" "$listing" | grep -Eq "(^|/)sbin/fsck.ext4$" || { echo "initramfs is missing ext4 fsck tools: $initrd" >&2; exit 1; }; done' \
    --run-command 'test -f /usr/share/guestos/fex-mesa/graphics_provider.json' \
    --run-command 'test -x /usr/local/share/varmint/steam-bootstrap/steamrtarm64/steam && test -L /usr/local/share/varmint/steam-bootstrap/steamrt64 && test -f /usr/local/share/varmint/steam-bootstrap/.steam-enable-steamrt64-client && grep -Fx publicbeta /usr/local/share/varmint/steam-bootstrap/package/beta >/dev/null' \
    --run-command 'test -x /usr/local/bin/steam-edge-resize && test -x /usr/local/bin/varmint-steam && test -x /usr/local/bin/varmint-display-scale && test -f /etc/xdg/autostart/varmint-display-scale.desktop && test -x /usr/local/libexec/varmint-host-time' \
    --run-command 'systemctl enable varmint-grow-root.service varmint-host-time.service varmint-firstboot.service' \
    --run-command 'passwd -l root' \
    --run-command 'printf "varmint\n" > /etc/hostname' \
    --run-command 'truncate -s 0 /etc/machine-id' \
    --run-command 'rm -f /var/lib/dbus/machine-id /etc/ssh/ssh_host_* /var/lib/varmint/firstboot.done /var/lib/varmint/firstboot.user /usr/local/libexec/varmint-provision' \
    --run-command 'rm -rf /var/lib/cloud/* /etc/systemd/system/getty@tty1.service.d /etc/systemd/system/serial-getty@*.service.d' \
    --run-command 'apt-get clean && rm -rf /var/lib/apt/lists/* /var/log/*.log /var/log/journal/*'

if virt-cat -a "$work_image" /etc/passwd | awk -F: '$3 >= 1000 && $3 < 65534 { found = 1 } END { exit !found }'; then
    echo "error: source image contains a regular user; expected a clean nocloud image" >&2
    exit 1
fi

mapfile -t roots < <(guestfish --ro -a "$work_image" <<'EOF_GUESTFISH'
run
inspect-os
EOF_GUESTFISH
)
[ "${#roots[@]}" -eq 1 ] || { echo "error: expected one root filesystem, found ${#roots[@]}" >&2; exit 1; }
root_device="${roots[0]}"
root_type="$(guestfish --ro -a "$work_image" <<EOF_GUESTFISH
run
vfs-type $root_device
EOF_GUESTFISH
)"
[ "$root_type" = ext4 ] || { echo "error: expected ext4 root at $root_device, found: $root_type" >&2; exit 1; }

guestfish --rw -a "$work_image" <<EOF_GUESTFISH
run
set-label $root_device varmint-root
EOF_GUESTFISH

boot_files="$(virt-ls -a "$work_image" /boot)"
kernel_name="$(printf '%s\n' "$boot_files" | grep '^vmlinuz-' | sort -V | tail -n 1)"
[ -n "$kernel_name" ] || { echo "error: no kernel found in /boot" >&2; exit 1; }
kernel_version="${kernel_name#vmlinuz-}"
initrd_name="initrd.img-$kernel_version"
printf '%s\n' "$boot_files" | grep -Fx "$initrd_name" >/dev/null \
    || { echo "error: missing matching initrd in /boot: $initrd_name" >&2; exit 1; }

virt-copy-out -a "$work_image" "/boot/$kernel_name" "$stage"
virt-copy-out -a "$work_image" "/boot/$initrd_name" "$stage"

PUBLISH_DIR="$(mktemp -d "$OUTPUT_DIR/.partial.XXXXXX")"
install -m 0644 "$stage/$kernel_name" "$PUBLISH_DIR/Image"
install -m 0644 "$stage/$initrd_name" "$PUBLISH_DIR/initrd"

zstd -T0 -19 -f "$work_image" -o "$PUBLISH_DIR/varmint-debian.raw.zst"
sha256sum \
    "$PUBLISH_DIR/Image" \
    "$PUBLISH_DIR/initrd" \
    "$PUBLISH_DIR/varmint-debian.raw.zst" \
    | sed "s#$PUBLISH_DIR/##" \
    > "$PUBLISH_DIR/SHA256SUMS"
printf 'url=%s\nsha512=%s\n' "${SOURCE_URL:-unknown}" "${SOURCE_SHA512:-unknown}" > "$PUBLISH_DIR/SOURCE"

if [ -n "${HOST_UID:-}" ] && [ -n "${HOST_GID:-}" ]; then
    chown "$HOST_UID:$HOST_GID" \
        "$PUBLISH_DIR/Image" \
        "$PUBLISH_DIR/initrd" \
        "$PUBLISH_DIR/varmint-debian.raw.zst" \
        "$PUBLISH_DIR/SHA256SUMS" \
        "$PUBLISH_DIR/SOURCE" 2>/dev/null || true
fi

for artifact in Image initrd varmint-debian.raw.zst SHA256SUMS SOURCE; do
    mv -f "$PUBLISH_DIR/$artifact" "$OUTPUT_DIR/$artifact"
done

printf '\nGuest runtime ready in %s\n' "$OUTPUT_DIR"
