FROM debian:trixie-slim AS fex-rootfs

ARG FEX_ROOTFS_URL=https://rootfs.fex-emu.gg/Ubuntu_24_04/2026-08-11/Ubuntu_24_04.sqsh
ARG FEX_ROOTFS_XXH3=3517e0e5ea25a473

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        squashfs-tools \
        xxhash && \
    rm -rf /var/lib/apt/lists/*

RUN curl \
        --fail \
        --location \
        --retry 3 \
        --retry-delay 2 \
        --output /tmp/fex-rootfs.sqsh \
        "$FEX_ROOTFS_URL" && \
    actual="$(xxhsum -H3 /tmp/fex-rootfs.sqsh | awk '{print $1}' | sed 's/^XXH3_//')" && \
    printf 'FEX RootFS XXH3 actual=%s expected=%s\\n' "$actual" "$FEX_ROOTFS_XXH3" && \
    test "$actual" = "$FEX_ROOTFS_XXH3" && \
    mkdir -p /opt/fex-rootfs && \
    unsquashfs -f -d /opt/fex-rootfs /tmp/fex-rootfs.sqsh >/dev/null && \
    printf '%s\\n' '=== FEX rootfs top-level ===' && \
    find /opt/fex-rootfs -maxdepth 2 -name graphics_provider.json -o -maxdepth 1 -type f -print && \
    test -f /opt/fex-rootfs/graphics_provider.json && \
    rm -f /tmp/fex-rootfs.sqsh

FROM debian:trixie-slim AS steam-helper-builder

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        gcc \
        libc6-dev \
        libx11-dev \
        libxi-dev && \
    rm -rf /var/lib/apt/lists/*

COPY guest/steam-edge-resize.c /src/steam-edge-resize.c

RUN gcc \
        -O2 \
        -Wall \
        -Wextra \
        -Werror \
        -o /usr/local/bin/steam-edge-resize \
        /src/steam-edge-resize.c \
        -lX11 \
        -lXi && \
    test -x /usr/local/bin/steam-edge-resize

FROM debian:trixie-slim AS steam-bootstrap

ARG STEAM_BRANCH=publicbeta
ARG STEAM_MANIFEST_BASE=https://client-update.steamstatic.com

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        unzip && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p /opt/steam-bootstrap/package && \
    curl \
        --fail \
        --location \
        --retry 3 \
        --retry-delay 2 \
        --output /tmp/steam-arm64.manifest \
        "$STEAM_MANIFEST_BASE/steam_client_${STEAM_BRANCH}_linuxarm64" && \
    payload="$(grep -E '"file"[[:space:]]+"bins_linuxarm64_linuxarm64\.zip' \
        /tmp/steam-arm64.manifest \
        | cut -d '"' -f4 \
        | head -1)" && \
    test -n "$payload" && \
    printf 'Steam ARM64 bootstrap payload: %s\n' "$payload" && \
    curl \
        --fail \
        --location \
        --retry 3 \
        --retry-delay 2 \
        --output /tmp/steam-arm64.zip \
        "$STEAM_MANIFEST_BASE/$payload" && \
    unzip -q /tmp/steam-arm64.zip -d /opt/steam-bootstrap && \
    test -f /opt/steam-bootstrap/steamrtarm64/steam && \
    chmod +x \
        /opt/steam-bootstrap/steamrtarm64/steam \
        /opt/steam-bootstrap/steamrtarm64/steamwebhelper \
        /opt/steam-bootstrap/steamrtarm64/steamwebhelper.sh \
        /opt/steam-bootstrap/steamrtarm64/gldriverquery \
        /opt/steam-bootstrap/steamrtarm64/vulkandriverquery \
        /opt/steam-bootstrap/steamrtarm64/steamsysinfo && \
    test -x /opt/steam-bootstrap/steamrtarm64/steam && \
    ln -sfn steamrtarm64 /opt/steam-bootstrap/steamrt64 && \
    touch /opt/steam-bootstrap/.steam-enable-steamrt64-client && \
    printf '%s\n' "$STEAM_BRANCH" > /opt/steam-bootstrap/package/beta && \
    rm -f /tmp/steam-arm64.manifest /tmp/steam-arm64.zip

FROM --platform=linux/amd64 debian:trixie-slim AS neptune-builder

ENV DEBIAN_FRONTEND=noninteractive

COPY guest/neptune/provision.sh /usr/local/bin/varmint-neptune-provision
COPY guest/neptune/versions.env /opt/varmint/neptune/versions.env
COPY guest/neptune/deps.txt /opt/varmint/neptune/deps.txt
COPY patches/neptune-protocol /opt/varmint/neptune/patches/neptune-protocol
COPY patches/neptune-generated /opt/varmint/neptune/patches/neptune-generated
COPY patches/virtio-win-mesa /opt/varmint/neptune/patches/virtio-win-mesa

RUN /usr/local/bin/varmint-neptune-provision && \
    test -f /usr/local/lib/varmint/neptune/x86_64-windows/d3d11.dll && \
    test -f /usr/local/lib/varmint/neptune/x86_64-windows/dxgi.dll && \
    test -f /usr/local/lib/varmint/neptune/x86_64-windows/d3d12.dll && \
    test -f /usr/local/lib/varmint/neptune/x86_64-windows/nptunix.dll && \
    test -f /usr/local/lib/varmint/neptune/x86_64-unix/nptunix.so


FROM debian:trixie-slim

ENV DEBIAN_FRONTEND=noninteractive \
    LIBGUESTFS_BACKEND=direct \
    LIBGUESTFS_BACKEND_SETTINGS=force_tcg

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        libguestfs-tools \
        linux-image-arm64 \
        ipxe-qemu \
        qemu-utils \
        zstd && \
    rm -rf /var/lib/apt/lists/*

COPY --from=fex-rootfs /opt/fex-rootfs /opt/varmint/rootfs/usr/share/guestos/fex-mesa
COPY --from=steam-bootstrap /opt/steam-bootstrap /opt/varmint/rootfs/usr/local/share/varmint/steam-bootstrap
COPY --from=steam-helper-builder /usr/local/bin/steam-edge-resize /opt/varmint/rootfs/usr/local/bin/steam-edge-resize
COPY --from=neptune-builder /usr/local/lib/varmint/neptune /opt/varmint/rootfs/usr/local/lib/varmint/neptune
COPY guest /opt/varmint/source
COPY guest/neptune/versions.env /opt/varmint/rootfs/usr/local/share/varmint/neptune/versions.env
COPY guest/neptune/deps.txt /opt/varmint/rootfs/usr/local/share/varmint/neptune/deps.txt
COPY patches/neptune-protocol /opt/varmint/rootfs/usr/local/share/varmint/neptune/patches/neptune-protocol
COPY patches/neptune-generated /opt/varmint/rootfs/usr/local/share/varmint/neptune/patches/neptune-generated
COPY patches/virtio-win-mesa /opt/varmint/rootfs/usr/local/share/varmint/neptune/patches/virtio-win-mesa

RUN install -Dm0755 /opt/varmint/source/build-image-container.sh /usr/local/bin/build-varmint-guest && \
    install -Dm0755 /opt/varmint/source/varmint-gpu-check /opt/varmint/rootfs/usr/local/bin/varmint-gpu-check && \
    install -Dm0755 /opt/varmint/source/varmint-steam /opt/varmint/rootfs/usr/local/bin/varmint-steam && \
    install -Dm0755 /opt/varmint/source/varmint-display-scale /opt/varmint/rootfs/usr/local/share/varmint/varmint-display-scale && \
    install -Dm0755 /opt/varmint/source/neptune/build.sh /opt/varmint/rootfs/usr/local/bin/varmint-neptune-build && \
    install -Dm0755 /opt/varmint/source/neptune/game-graphics.sh /opt/varmint/rootfs/usr/local/bin/varmint-game-graphics && \
    install -Dm0755 /opt/varmint/source/neptune/dev-setup.sh /opt/varmint/rootfs/usr/local/bin/varmint-neptune-dev-setup && \
    install -Dm0755 /opt/varmint/source/varmint-clipboard-agent /opt/varmint/rootfs/usr/local/libexec/varmint-clipboard-agent && \
    install -Dm0755 /opt/varmint/source/varmint-audio-watchdog /opt/varmint/rootfs/usr/local/libexec/varmint-audio-watchdog && \
    install -Dm0755 /opt/varmint/source/varmint-host-time /opt/varmint/rootfs/usr/local/libexec/varmint-host-time && \
    install -Dm0755 /opt/varmint/source/varmint-firstboot /opt/varmint/rootfs/usr/local/libexec/varmint-firstboot && \
    install -Dm0755 /opt/varmint/source/varmint-grow-root /opt/varmint/rootfs/usr/local/libexec/varmint-grow-root && \
    install -Dm0755 /opt/varmint/source/varmint-provision /opt/varmint/rootfs/usr/local/libexec/varmint-provision && \
    install -Dm0755 /opt/varmint/source/varmint-initramfs-fsck-hook /opt/varmint/rootfs/etc/initramfs-tools/hooks/varmint-fsck && \
    install -Dm0644 /opt/varmint/source/varmint-host-time.service /opt/varmint/rootfs/etc/systemd/system/varmint-host-time.service && \
    install -Dm0644 /opt/varmint/source/varmint-audio-watchdog.service /opt/varmint/rootfs/etc/systemd/user/varmint-audio-watchdog.service && \
    install -Dm0644 /opt/varmint/source/varmint-firstboot.service /opt/varmint/rootfs/etc/systemd/system/varmint-firstboot.service && \
    install -Dm0644 /opt/varmint/source/varmint-grow-root.service /opt/varmint/rootfs/etc/systemd/system/varmint-grow-root.service && \
    install -Dm0644 /opt/varmint/source/varmint-clipboard.desktop /opt/varmint/rootfs/etc/xdg/autostart/varmint-clipboard.desktop && \
    install -Dm0644 /opt/varmint/source/steam-edge-resize.desktop /opt/varmint/rootfs/etc/xdg/autostart/steam-edge-resize.desktop && \
    install -Dm0644 /opt/varmint/source/varmint-display-scale.desktop /opt/varmint/rootfs/usr/local/share/varmint/varmint-display-scale.desktop && \
    install -Dm0644 /opt/varmint/source/steam.desktop /opt/varmint/rootfs/usr/local/share/applications/steam.desktop && \
    rm -rf /opt/varmint/source

ENTRYPOINT ["/usr/local/bin/build-varmint-guest"]
