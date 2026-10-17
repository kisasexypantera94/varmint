#!/usr/bin/env bash
set -euo pipefail

SOURCE_IMAGE="${1:?source image path is required}"
SCRIPT_DIR=/opt/varmint
OUTPUT_DIR=/output
WORK_DIR="$(mktemp -d /work/varmint.XXXXXXXX)"
WORK_IMAGE="$WORK_DIR/varmint-debian.raw"
MOUNT_DIR="$WORK_DIR/root"
PUBLISH_DIR=""
LOOP=""
RESOLV_BACKUP="$WORK_DIR/resolv.conf.original"
RESOLV_PRESENT=0
POLICY_PRESENT=0
POLICY_BACKUP="$WORK_DIR/policy-rc.d.original"

cleanup() {
  if mountpoint -q "$MOUNT_DIR"; then
    if ! umount -R "$MOUNT_DIR"; then
      echo "ERROR: cannot unmount $MOUNT_DIR; inspect mounts before cleanup" >&2
      return
    fi
  fi
  if [ -n "$LOOP" ]; then
    losetup -d "$LOOP" || true
  fi
  [ -z "$PUBLISH_DIR" ] || rm -rf "$PUBLISH_DIR"
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

attach() {
  LOOP="$(losetup --find --show --partscan "$WORK_IMAGE")"
  echo "loop device: $LOOP"

  loop_name=${LOOP##*/}
  for part in 1 15; do
    node="${LOOP}p${part}"
    sysdev="/sys/class/block/${loop_name}/${loop_name}p${part}/dev"

    i=0
    while [ ! -r "$sysdev" ] && [ "$i" -lt 50 ]; do
      sleep 0.1
      i=$((i + 1))
    done
    [ -r "$sysdev" ] || {
      echo "partition $part did not appear for $LOOP" >&2
      exit 1
    }

    devnum="$(cat "$sysdev")"
    rm -f "$node"
    mknod "$node" b "${devnum%%:*}" "${devnum##*:}"
  done

  test -b "${LOOP}p1"
  test -b "${LOOP}p15"
}

echo '=== Convert Debian QCOW2 to RAW ==='
qemu-img convert -f qcow2 -O raw -S 4k "$SOURCE_IMAGE" "$WORK_IMAGE"
truncate -s 8G "$WORK_IMAGE"
attach

echo '=== Expand GPT and ext4 ==='
sgdisk -e "$LOOP"
growpart "$LOOP" 1
losetup -d "$LOOP"
LOOP=""
attach

# e2fsck returns 1 after fixing errors.
set +e
e2fsck -f -y "${LOOP}p1"
fsck_status=$?
set -e
[ "$fsck_status" -le 1 ] || exit "$fsck_status"
resize2fs "${LOOP}p1"
e2label "${LOOP}p1" varmint-root

mkdir -p "$MOUNT_DIR"
mount -t ext4 "${LOOP}p1" "$MOUNT_DIR"
# Varmint boots the kernel directly.
mkdir -p "$MOUNT_DIR/boot/efi"

cp -a "$SCRIPT_DIR/rootfs/." "$MOUNT_DIR/"

# Use Docker DNS inside the chroot.
if [ -e "$MOUNT_DIR/etc/resolv.conf" ] || [ -L "$MOUNT_DIR/etc/resolv.conf" ]; then
  cp -a "$MOUNT_DIR/etc/resolv.conf" "$RESOLV_BACKUP"
  RESOLV_PRESENT=1
fi
rm -f "$MOUNT_DIR/etc/resolv.conf"
cp /etc/resolv.conf "$MOUNT_DIR/etc/resolv.conf"

# Prevent package scripts from starting services.
if [ -e "$MOUNT_DIR/usr/sbin/policy-rc.d" ] || [ -L "$MOUNT_DIR/usr/sbin/policy-rc.d" ]; then
  cp -a "$MOUNT_DIR/usr/sbin/policy-rc.d" "$POLICY_BACKUP"
  POLICY_PRESENT=1
fi
cat > "$MOUNT_DIR/usr/sbin/policy-rc.d" <<'EOF_POLICY'
#!/bin/sh
exit 101
EOF_POLICY
chmod +x "$MOUNT_DIR/usr/sbin/policy-rc.d"

mount -t proc proc "$MOUNT_DIR/proc"
mount --rbind /dev "$MOUNT_DIR/dev"
mount --make-rslave "$MOUNT_DIR/dev"
mount --rbind /sys "$MOUNT_DIR/sys"
mount --make-rslave "$MOUNT_DIR/sys"
mount --bind /run "$MOUNT_DIR/run"

echo '=== Guest provisioning ==='
chroot "$MOUNT_DIR" /usr/bin/env DEBIAN_FRONTEND=noninteractive \
  /usr/local/libexec/varmint-provision

echo '=== Validate and sanitize ==='
chroot "$MOUNT_DIR" /bin/sh -eu -c '
  missing=""
  for cmd in curl e2fsck fsck growpart resize2fs sgdisk sudo glxinfo pactl pulseaudio python3 update-initramfs vulkaninfo xclip; do
    command -v "$cmd" >/dev/null 2>&1 || missing="$missing $cmd"
  done
  [ -z "$missing" ] || { echo "missing guest commands:$missing" >&2; exit 1; }
  for initrd in /boot/initrd.img-*; do
    listing="$(lsinitramfs "$initrd")"
    printf "%s\n" "$listing" | grep -Eq "(^|/)sbin/fsck$"
    printf "%s\n" "$listing" | grep -Eq "(^|/)sbin/e2fsck$"
    printf "%s\n" "$listing" | grep -Eq "(^|/)sbin/fsck.ext4$"
  done
  test -f /usr/share/guestos/fex-mesa/graphics_provider.json
  test -x /usr/local/share/varmint/steam-bootstrap/steamrtarm64/steam
  test -L /usr/local/share/varmint/steam-bootstrap/steamrt64
  test -f /usr/local/share/varmint/steam-bootstrap/.steam-enable-steamrt64-client
  grep -Fx publicbeta /usr/local/share/varmint/steam-bootstrap/package/beta
  test -x /usr/local/bin/steam-edge-resize
  test -x /usr/local/bin/varmint-steam
  test -x /usr/local/bin/varmint-display-scale
  test -f /etc/xdg/autostart/varmint-display-scale.desktop
  test -x /usr/local/libexec/varmint-host-time
  test -f /etc/systemd/system/sddm.service.d/10-varmint-home.conf
  systemctl enable varmint-grow-root.service varmint-host-time.service varmint-home.service varmint-firstboot.service varmint-user-state.path
  passwd -l root
  printf "varmint\n" > /etc/hostname
  truncate -s 0 /etc/machine-id
  rm -f /var/lib/dbus/machine-id /etc/ssh/ssh_host_* \
      /var/lib/varmint/firstboot.done /var/lib/varmint/firstboot.user \
      /usr/local/libexec/varmint-provision
  rm -rf /var/lib/cloud/* /etc/systemd/system/getty@tty1.service.d \
      /etc/systemd/system/serial-getty@*.service.d
  apt-get clean
  rm -rf /var/lib/apt/lists/* /var/log/*.log /var/log/journal/*
'

if chroot "$MOUNT_DIR" /bin/sh -c \
  "awk -F: '\$3 >= 1000 && \$3 < 65534 { found = 1 } END { exit !found }' /etc/passwd"; then
  echo 'error: image contains a regular user' >&2
  exit 1
fi

if [ "$RESOLV_PRESENT" -eq 1 ]; then
  cp -a "$RESOLV_BACKUP" "$MOUNT_DIR/etc/resolv.conf.original-for-restore"
  rm -f "$MOUNT_DIR/etc/resolv.conf"
  mv "$MOUNT_DIR/etc/resolv.conf.original-for-restore" "$MOUNT_DIR/etc/resolv.conf"
else
  rm -f "$MOUNT_DIR/etc/resolv.conf"
fi
rm -f "$MOUNT_DIR/usr/sbin/policy-rc.d"
if [ "$POLICY_PRESENT" -eq 1 ]; then
  cp -a "$POLICY_BACKUP" "$MOUNT_DIR/usr/sbin/policy-rc.d"
fi

mkdir -p "$OUTPUT_DIR"
PUBLISH_DIR="$(mktemp -d "$OUTPUT_DIR/.partial.XXXXXX")"
kernel_name="$(find "$MOUNT_DIR/boot" -maxdepth 1 -type f -name 'vmlinuz-*' -printf '%f\n' | sort -V | tail -n 1)"
[ -n "$kernel_name" ] || { echo 'kernel missing from guest /boot' >&2; exit 1; }
initrd_name="initrd.img-${kernel_name#vmlinuz-}"
test -f "$MOUNT_DIR/boot/$initrd_name"
cp "$MOUNT_DIR/boot/$kernel_name" "$PUBLISH_DIR/Image"
cp "$MOUNT_DIR/boot/$initrd_name" "$PUBLISH_DIR/initrd"

umount -R "$MOUNT_DIR"
losetup -d "$LOOP"
LOOP=""

echo '=== Compress guest image ==='
zstd -T0 -19 -f "$WORK_IMAGE" -o "$PUBLISH_DIR/varmint-debian.raw.zst"
sha256sum "$PUBLISH_DIR/Image" "$PUBLISH_DIR/initrd" \
  "$PUBLISH_DIR/varmint-debian.raw.zst" |
  sed "s#$PUBLISH_DIR/##" > "$PUBLISH_DIR/SHA256SUMS"
awk '$2 == "varmint-debian.raw.zst" { print $1 }' "$PUBLISH_DIR/SHA256SUMS" > "$PUBLISH_DIR/system-image-id"
[ "$(wc -c < "$PUBLISH_DIR/system-image-id" | tr -d ' ')" -eq 65 ] || { echo "invalid system image id" >&2; exit 1; }
printf 'url=%s\nsha512=%s\n' "${SOURCE_URL:-unknown}" \
  "${SOURCE_SHA512:-unknown}" > "$PUBLISH_DIR/SOURCE"

if [ -n "${HOST_UID:-}" ] && [ -n "${HOST_GID:-}" ]; then
  chown "$HOST_UID:$HOST_GID" "$PUBLISH_DIR"/* 2>/dev/null || true
fi
for artifact in Image initrd varmint-debian.raw.zst system-image-id SHA256SUMS SOURCE; do
  mv -f "$PUBLISH_DIR/$artifact" "$OUTPUT_DIR/$artifact"
done

echo "Guest image ready in $OUTPUT_DIR"
