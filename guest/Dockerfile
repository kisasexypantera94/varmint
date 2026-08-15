ARG FEX_COMMIT=99662b70ffbf24a9ad50f0317f75166e0822f5a4

FROM debian:trixie-slim AS fex-builder

ARG FEX_COMMIT
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        cmake \
        git \
        libasound2-dev \
        libc6-dev-amd64-cross \
        libc6-dev-i386-cross \
        libclang-19-dev \
        libdrm-dev \
        libegl-dev \
        libgl-dev \
        libssl-dev \
        libstdc++-14-dev-amd64-cross \
        libstdc++-14-dev-i386-cross \
        libwayland-dev \
        libx11-dev \
        libxcb1-dev \
        libxrandr-dev \
        libxrender-dev \
        lld \
        llvm-19-dev \
        ninja-build \
        pkgconf && \
    rm -rf /var/lib/apt/lists/*

RUN git clone https://github.com/FEX-Emu/FEX.git /src/FEX && \
    git -C /src/FEX checkout "$FEX_COMMIT" && \
    git -C /src/FEX submodule update --init --recursive && \
    sed -i '/Opening host-side X11 display:/s/fprintf/\/\/ fprintf/' /src/FEX/ThunkLibs/include/common/X11Manager.h && \
    sed -i '/Linking address %p to host invoker/s/fprintf/\/\/ fprintf/' /src/FEX/ThunkLibs/libvulkan/Guest.cpp

RUN cmake \
        -S /src/FEX \
        -B /src/FEX/build \
        -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_C_COMPILER=/usr/bin/clang-19 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-19 \
        -DCMAKE_INSTALL_PREFIX=/usr \
        -DCMAKE_INSTALL_LIBDIR=lib/aarch64-linux-gnu \
        -DBUILD_FEXCONFIG=OFF \
        -DBUILD_FEX_LINUX_TESTS=OFF \
        -DBUILD_TESTING=OFF \
        -DBUILD_THUNKS=ON \
        -DENABLE_ASSERTIONS=OFF \
        -DENABLE_CLANG_THUNKS=ON \
        -DTUNE_ARCH=generic \
        -DTUNE_CPU=generic \
        -DUSE_LINKER=lld \
        -DX86_DEV_ROOTFS=/ && \
    cmake --build /src/FEX/build --parallel && \
    DESTDIR=/opt/fex-install cmake --install /src/FEX/build && \
    test -x /opt/fex-install/usr/bin/FEXBash && \
    test -x /opt/fex-install/usr/bin/FEXRootFSFetcher && \
    test -f /opt/fex-install/usr/share/fex-emu/GuestThunks/libvulkan-guest.so && \
    test -f /opt/fex-install/usr/lib/aarch64-linux-gnu/fex-emu/HostThunks/libvulkan-host.so && \
    test -f /opt/fex-install/usr/lib/aarch64-linux-gnu/fex-emu/HostThunks_32/libvulkan-host.so

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

COPY --from=fex-builder /opt/fex-install /opt/varmint/fex-install
COPY . /opt/varmint/source

RUN install -Dm0755 /opt/varmint/source/build-image-container.sh /usr/local/bin/build-varmint-guest && \
    install -Dm0755 /opt/varmint/source/varmint-gpu-check /opt/varmint/rootfs/usr/local/bin/varmint-gpu-check && \
    install -Dm0755 /opt/varmint/source/varmint-steam /opt/varmint/rootfs/usr/local/bin/varmint-steam && \
    install -Dm0755 /opt/varmint/source/varmint-clipboard-agent /opt/varmint/rootfs/usr/local/libexec/varmint-clipboard-agent && \
    install -Dm0755 /opt/varmint/source/varmint-firstboot /opt/varmint/rootfs/usr/local/libexec/varmint-firstboot && \
    install -Dm0755 /opt/varmint/source/varmint-grow-root /opt/varmint/rootfs/usr/local/libexec/varmint-grow-root && \
    install -Dm0755 /opt/varmint/source/varmint-provision /opt/varmint/rootfs/usr/local/libexec/varmint-provision && \
    install -Dm0755 /opt/varmint/source/varmint-initramfs-fsck-hook /opt/varmint/rootfs/etc/initramfs-tools/hooks/varmint-fsck && \
    install -Dm0644 /opt/varmint/source/varmint-firstboot.service /opt/varmint/rootfs/etc/systemd/system/varmint-firstboot.service && \
    install -Dm0644 /opt/varmint/source/varmint-grow-root.service /opt/varmint/rootfs/etc/systemd/system/varmint-grow-root.service && \
    install -Dm0644 /opt/varmint/source/varmint-clipboard.desktop /opt/varmint/rootfs/etc/xdg/autostart/varmint-clipboard.desktop && \
    install -Dm0644 /opt/varmint/source/steam.desktop /opt/varmint/rootfs/usr/local/share/applications/steam.desktop && \
    rm -rf /opt/varmint/source

ENTRYPOINT ["/usr/local/bin/build-varmint-guest"]
