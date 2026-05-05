#!/usr/bin/env bash
set -euo pipefail

BUSYBOX_DIR="${1:-busybox}"

if [ ! -d "$BUSYBOX_DIR" ]; then
  echo "busybox source dir not found: $BUSYBOX_DIR" >&2
  exit 1
fi

docker run --rm -i -v "$PWD/$BUSYBOX_DIR:/src" -w /src alpine:3.18 sh -eux <<'EOF'
apk add --no-cache build-base linux-headers bash cpio gzip

# wipe any half-finished previous attempt
make distclean || true

# fresh defconfig
make defconfig

# enable static linking, disable broken SHA-NI hwaccel that fails on master
sed -i 's|# CONFIG_STATIC is not set|CONFIG_STATIC=y|'           .config
sed -i 's|CONFIG_SHA1_HWACCEL=y|# CONFIG_SHA1_HWACCEL is not set|' .config
sed -i 's|CONFIG_SHA256_HWACCEL=y|# CONFIG_SHA256_HWACCEL is not set|' .config
yes "" | make oldconfig >/dev/null

# single-threaded build so any error stops immediately and is visible
make

# sanity check: the binary really must exist before we proceed
test -x busybox || { echo "FATAL: busybox binary was not produced"; exit 1; }

# build initramfs tree
rm -rf _initramfs
mkdir -p _initramfs/bin _initramfs/sbin _initramfs/proc _initramfs/sys \
         _initramfs/tmp _initramfs/dev _initramfs/etc _initramfs/root
cp busybox _initramfs/bin/busybox
ln -sf busybox _initramfs/bin/sh
( cd _initramfs && ./bin/busybox --install bin )


# init script
cat > _initramfs/init <<'INIT'
#!/bin/sh
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
mkdir -p /proc /sys /tmp /dev
mount -t proc     none /proc
mount -t sysfs    none /sys
mount -t tmpfs    none /tmp
mount -t devtmpfs none /dev 2>/dev/null || true
echo "Welcome to Linux"
exec /bin/sh
INIT
chmod +x _initramfs/init

# pack
( cd _initramfs && find . -print0 | cpio --null -o -H newc ) | gzip -9 > /src/initramfs.cpio.gz
ls -lh /src/initramfs.cpio.gz
EOF

echo
echo "Done: $BUSYBOX_DIR/initramfs.cpio.gz"