#!/usr/bin/env bash
set -euo pipefail

SOURCE_IMAGE="${1:-}"
OUTPUT_DIR=/output
SCRIPT_DIR=/opt/varmint
PUBLISH_DIR="$OUTPUT_DIR/.partial.$$"

[ -n "$SOURCE_IMAGE" ] || { echo "error: source image path is required" >&2; exit 1; }
[ -f "$SOURCE_IMAGE" ] || { echo "error: missing source image: $SOURCE_IMAGE" >&2; exit 1; }

work_dir="$(mktemp -d /tmp/varmint-guest.XXXXXX)"
trap 'rm -rf "$work_dir" "$PUBLISH_DIR"' EXIT
work_image="$work_dir/varmint-debian.raw"
stage="$work_dir/stage"
rm -rf "$PUBLISH_DIR"
mkdir -p "$OUTPUT_DIR" "$PUBLISH_DIR" "$stage"

qemu-img convert -p -O raw "$SOURCE_IMAGE" "$work_image"

virt-customize --network -a "$work_image" \
    --mkdir /usr/local/libexec \
    --mkdir /usr/local/share/applications \
    --mkdir /etc/xdg/autostart \
    --copy-in "$SCRIPT_DIR/fex-install:/tmp" \
    --copy-in "$SCRIPT_DIR/varmint-provision:/tmp" \
    --copy-in "$SCRIPT_DIR/varmint-gpu-check:/usr/local/bin" \
    --copy-in "$SCRIPT_DIR/varmint-grow-root:/usr/local/libexec" \
    --copy-in "$SCRIPT_DIR/varmint-firstboot:/usr/local/libexec" \
    --copy-in "$SCRIPT_DIR/varmint-clipboard-agent:/usr/local/libexec" \
    --copy-in "$SCRIPT_DIR/varmint-clipboard.desktop:/etc/xdg/autostart" \
    --copy-in "$SCRIPT_DIR/varmint-steam:/usr/local/bin" \
    --copy-in "$SCRIPT_DIR/steam.desktop:/usr/local/share/applications" \
    --copy-in "$SCRIPT_DIR/varmint-grow-root.service:/etc/systemd/system" \
    --copy-in "$SCRIPT_DIR/varmint-firstboot.service:/etc/systemd/system" \
    --copy-in "$SCRIPT_DIR/varmint-gaming-setup.service:/etc/systemd/system" \
    --run-command 'cp -a /tmp/fex-install/. / && rm -rf /tmp/fex-install' \
    --run-command '/tmp/varmint-provision' \
    --run-command 'missing=""; for command in FEXBash FEXRootFSFetcher curl growpart resize2fs sgdisk sudo glxinfo pactl pulseaudio python3 unsquashfs vulkaninfo xclip; do command -v "$command" >/dev/null 2>&1 || missing="$missing $command"; done; [ -z "$missing" ] || { echo "missing required guest commands:$missing" >&2; exit 1; }' \
    --run-command 'test -f /usr/share/fex-emu/GuestThunks/libvulkan-guest.so && test -f /usr/lib/aarch64-linux-gnu/fex-emu/HostThunks/libvulkan-host.so && test -f /usr/lib/aarch64-linux-gnu/fex-emu/HostThunks_32/libvulkan-host.so' \
    --run-command 'chmod 0755 /usr/local/bin/varmint-gpu-check /usr/local/bin/varmint-steam /usr/local/libexec/varmint-grow-root /usr/local/libexec/varmint-firstboot /usr/local/libexec/varmint-clipboard-agent' \
    --run-command 'systemctl enable varmint-grow-root.service varmint-firstboot.service varmint-gaming-setup.service' \
    --run-command 'passwd -l root' \
    --run-command 'printf "varmint\n" > /etc/hostname' \
    --run-command 'truncate -s 0 /etc/machine-id' \
    --run-command 'rm -f /var/lib/dbus/machine-id /etc/ssh/ssh_host_* /var/lib/varmint/firstboot.done /var/lib/varmint/gaming-setup.done /var/lib/varmint/firstboot.user /tmp/varmint-provision' \
    --run-command 'rm -rf /var/lib/cloud/* /etc/systemd/system/getty@tty1.service.d /etc/systemd/system/serial-getty@*.service.d' \
    --run-command 'apt-get clean' \
    --run-command 'rm -rf /var/lib/apt/lists/* /var/log/*.log /var/log/journal/*'

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
