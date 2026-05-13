#!/usr/bin/env bash
set -euo pipefail
BUSYBOX_DIR="${1:-busybox}"
# kernel package version, e.g. 6.12.73+deb13-arm64
# look it up at https://packages.debian.org/trixie/arm64/  (search "linux-image-")
KVER="${2:-6.12.73+deb13-arm64}"
DEB_URL="${3:-}"

if [ ! -d "$BUSYBOX_DIR" ]; then
  echo "busybox source dir not found: $BUSYBOX_DIR" >&2
  exit 1
fi

docker run --rm -i \
  -v "$PWD/$BUSYBOX_DIR:/src" \
  -e KVER="$KVER" \
  -e DEB_URL="$DEB_URL" \
  -w /src alpine:3.18 sh -eux <<'EOF'
# build deps + tools to fetch and unpack debian package and compressed modules
apk add --no-cache build-base linux-headers bash cpio gzip xz zstd \
                   curl dpkg binutils tar

# --- 1. build static busybox -------------------------------------------------
make distclean || true
make defconfig
sed -i 's|# CONFIG_STATIC is not set|CONFIG_STATIC=y|'                .config
sed -i 's|CONFIG_SHA1_HWACCEL=y|# CONFIG_SHA1_HWACCEL is not set|'    .config
sed -i 's|CONFIG_SHA256_HWACCEL=y|# CONFIG_SHA256_HWACCEL is not set|' .config
yes "" | make oldconfig >/dev/null
make
test -x busybox || { echo "FATAL: busybox binary was not produced"; exit 1; }

# --- 2. fetch and unpack the debian linux-image package ----------------------
rm -rf _pkg && mkdir _pkg
cd _pkg

if [ -z "${DEB_URL}" ]; then
    # Resolve via snapshot.debian.org JSON API. Package name is ABI-locked
    # (e.g. linux-image-6.12.73+deb13-arm64), so KVER is baked into the name —
    # any version returned should match. We still print everything for sanity.
    apk add --no-cache jq >/dev/null
    PKG="linux-image-${KVER}"
    META_URL="https://snapshot.debian.org/mr/binary/${PKG}/"
    echo "Looking up versions at: $META_URL"
    META_JSON="$(curl -fsSL "$META_URL")"
    echo "Available versions:"
    echo "$META_JSON" | jq -r '.result[] | "  " + .binary_version'

    # Take the most recent version; if there's only one, that's it.
    VERSION="$(echo "$META_JSON" | jq -r '.result[0].binary_version')"
    if [ -z "$VERSION" ] || [ "$VERSION" = "null" ]; then
        echo "FATAL: snapshot.debian.org has no record of $PKG" >&2
        exit 1
    fi
    echo "Picking version: $VERSION"

    BIN_META="$(curl -fsSL "https://snapshot.debian.org/mr/binary/${PKG}/${VERSION}/binfiles?fileinfo=1")"
    HASH="$(echo "$BIN_META" | jq -r '.result[] | select(.architecture=="arm64") | .hash' | head -1)"
    if [ -z "$HASH" ] || [ "$HASH" = "null" ]; then
        echo "FATAL: no arm64 build found for $PKG $VERSION" >&2
        echo "binfiles response:" >&2
        echo "$BIN_META" | jq . >&2
        exit 1
    fi
    DEB_URL="https://snapshot.debian.org/file/${HASH}"
    echo "Resolved -> $DEB_URL"
fi

echo "Fetching: $DEB_URL"
curl -fL -o pkg.deb "$DEB_URL"
DEB="pkg.deb"

# extract data.tar.* from the .deb (ar archive) and unpack it
ar x "$DEB"
mkdir root
case "$(ls data.tar.*)" in
    data.tar.xz)  tar -xJf data.tar.xz  -C root ;;
    data.tar.zst) tar --zstd -xf data.tar.zst -C root ;;
    data.tar.gz)  tar -xzf data.tar.gz  -C root ;;
    *)            echo "unknown data.tar format" >&2; exit 1 ;;
esac

MODDIR=""
for cand in "root/lib/modules/${KVER}" "root/usr/lib/modules/${KVER}"; do
    if [ -d "$cand" ]; then MODDIR="$cand"; break; fi
done
if [ -z "$MODDIR" ]; then
    echo "WARN: modules dir for ${KVER} not found, package contains:" >&2
    find root -maxdepth 4 -type d -name 'modules' >&2 || true
    # auto-pick if there's exactly one kernel inside
    ACTUAL_DIR="$(find root -type d -path '*/lib/modules/*' -maxdepth 5 -mindepth 4 | head -1)"
    if [ -n "$ACTUAL_DIR" ]; then
        echo "Using $ACTUAL_DIR"
        MODDIR="$ACTUAL_DIR"
        KVER="$(basename "$ACTUAL_DIR")"
    else
        echo "FATAL: no modules dir in package" >&2
        exit 1
    fi
fi
echo "Modules at: $MODDIR (KVER=$KVER)"
ABS_MODDIR="$(realpath "$MODDIR")"
cd /src

# --- 3. assemble initramfs tree ---------------------------------------------
rm -rf _initramfs
mkdir -p _initramfs/bin _initramfs/sbin _initramfs/proc _initramfs/sys \
         _initramfs/tmp _initramfs/dev _initramfs/etc _initramfs/root \
         "_initramfs/lib/modules/${KVER}"
cp busybox _initramfs/bin/busybox
ln -sf busybox _initramfs/bin/sh
( cd _initramfs && ./bin/busybox --install bin )

# --- 4. copy ONLY the modules we need, decompressing on the fly -------------
# Keep this list minimal — full initramfs must fit into VMM's initrd memory area.
WANTED='virtio virtio_ring virtio_mmio virtio_pci virtio_pci_modern_dev virtio_pci_legacy_dev virtio_blk virtio_net net_failover failover'
for m in $WANTED; do
    # find any of: foo.ko, foo.ko.xz, foo.ko.zst
    src="$(find "$ABS_MODDIR" -type f \
        \( -name "${m}.ko" -o -name "${m}.ko.xz" -o -name "${m}.ko.zst" \) | head -1)"
    if [ -z "$src" ]; then
        echo "  $m: not in package (probably builtin)"
        continue
    fi
    rel="${src#$ABS_MODDIR/}"
    case "$rel" in
      *.ko)     dst_rel="$rel" ;;
      *.ko.xz)  dst_rel="${rel%.xz}" ;;
      *.ko.zst) dst_rel="${rel%.zst}" ;;
    esac
    dst="_initramfs/lib/modules/${KVER}/$dst_rel"
    mkdir -p "$(dirname "$dst")"
    case "$rel" in
      *.ko)     cp       "$src" "$dst" ;;
      *.ko.xz)  xz   -dc "$src" > "$dst" ;;
      *.ko.zst) zstd -dq "$src" -o "$dst" ;;
    esac
    echo "  $m: $rel -> $dst_rel"
done

echo "=== bundled virtio modules (decompressed) ==="
find "_initramfs/lib/modules/${KVER}" -name '*.ko' | sort

# --- 5. init script ---------------------------------------------------------
cat > _initramfs/init <<'INIT'
#!/bin/sh
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
mkdir -p /proc /sys /tmp /dev
mount -t proc     none /proc
mount -t sysfs    none /sys
mount -t tmpfs    none /tmp
mount -t devtmpfs none /dev 2>/dev/null || true

echo "=== loading virtio stack ==="
echo "--- /lib/modules contents ---"
find /lib/modules -name '*.ko' 2>/dev/null | head -30
echo "-----------------------------"
# Load order matters: core -> ring -> transport -> class driver.
for m in virtio virtio_ring virtio_mmio \
         virtio_pci_modern_dev virtio_pci_legacy_dev virtio_pci \
         failover net_failover \
         virtio_blk virtio_net; do
    f=$(find /lib/modules -name "${m}.ko" 2>/dev/null | head -1)
    if [ -n "$f" ]; then
        echo "insmod $m"
        insmod "$f" || echo "  -> FAILED"
    else
        # virtio/virtio_ring may already be builtin; that's fine
        echo "skip $m (not bundled, may be builtin)"
    fi
done

sleep 1
echo "=== /sys/bus/virtio/devices ==="
ls /sys/bus/virtio/devices/ 2>/dev/null || echo "(empty)"
echo "=== /dev/vd* ==="
ls /dev/vd* 2>/dev/null || echo "(none)"

echo "Welcome to Linux"
exec /bin/sh
INIT
chmod +x _initramfs/init

# --- 6. pack ----------------------------------------------------------------
rm -rf _pkg
( cd _initramfs && find . -print0 | cpio --null -o -H newc --quiet ) \
  | gzip -9 > /src/initramfs.cpio.gz
ls -lh /src/initramfs.cpio.gz
EOF

echo
echo "Done: $BUSYBOX_DIR/initramfs.cpio.gz"