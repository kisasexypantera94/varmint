PREFIX_UPDATE_ACTIVE=0
PREFIX_BACKUP=""
PREFIX_HAD_EXISTING=0

rollback_dependency_prefix() {
  [ "$PREFIX_UPDATE_ACTIVE" = 1 ] || return 0

  rm -rf "$PREFIX"
  if [ "$PREFIX_HAD_EXISTING" = 1 ] && [ -e "$PREFIX_BACKUP" ]; then
    mv "$PREFIX_BACKUP" "$PREFIX"
    printf 'restored previous dependency prefix: %s\n' "$PREFIX" >&2
  fi
  PREFIX_UPDATE_ACTIVE=0
}

cleanup() {
  local status=$?
  trap - EXIT
  rollback_dependency_prefix || true
  exit "$status"
}

begin_dependency_prefix_update() {
  local parent basename
  parent="$(dirname "$PREFIX")"
  basename="$(basename "$PREFIX")"
  PREFIX_BACKUP="$parent/.${basename}.backup.$$"
  PREFIX_HAD_EXISTING=0

  mkdir -p "$parent"
  rm -rf "$PREFIX_BACKUP"
  if [ -e "$PREFIX" ]; then
    mv "$PREFIX" "$PREFIX_BACKUP"
    PREFIX_HAD_EXISTING=1
  fi
  PREFIX_UPDATE_ACTIVE=1
  mkdir -p "$PREFIX"
}

commit_dependency_prefix_update() {
  PREFIX_UPDATE_ACTIVE=0
  rm -rf "$PREFIX_BACKUP"
}

meson_install() {
  local source="$1"
  shift
  rm -rf "$source/build"
  (
    cd "$source"
    PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" \
    CFLAGS="-I$PREFIX/include" \
    CPPFLAGS="-I$PREFIX/include" \
    LDFLAGS="-L$PREFIX/lib -Wl,-rpath,$PREFIX/lib" \
      meson setup build --prefix "$PREFIX" "$@"
    meson compile -C build --verbose
    meson install -C build
  )
}

write_macos_x86_64_cross_file() {
  local path="$1"

  cat >"$path" <<'EOF'
[binaries]
c = ['clang', '-arch', 'x86_64']
cpp = ['clang++', '-arch', 'x86_64']
objc = ['clang', '-arch', 'x86_64']
objcpp = ['clang++', '-arch', 'x86_64']
ar = 'ar'
strip = 'strip'
pkg-config = 'pkg-config'

[host_machine]
system = 'darwin'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'

[properties]
needs_exe_wrapper = false
EOF
}

build_angle() {
  local source="$DEPS_SRC/WebKit"
  local angle="$source/Source/ThirdParty/ANGLE"
  local dylib

  log "ANGLE"
  checkout_repo "$ANGLE_REPOSITORY" "$ANGLE_COMMIT" "$source"
  [ -d "$angle" ] || die "missing ANGLE source directory: $angle"
  rm -rf "$angle/ANGLE.xcarchive"
  (
    cd "$angle"
    clean_env xcodebuild archive \
      -archivePath ANGLE \
      -scheme ANGLE \
      -sdk "$SDK" \
      -arch "$ARCH" \
      -configuration "$CONFIGURATION" \
      WEBCORE_LIBRARY_DIR=/usr/local/lib \
      NORMAL_UMBRELLA_FRAMEWORKS_DIR= \
      CODE_SIGNING_ALLOWED=NO \
      IPHONEOS_DEPLOYMENT_TARGET=14.0 \
      MACOSX_DEPLOYMENT_TARGET="$MINIMUM_MACOS" \
      XROS_DEPLOYMENT_TARGET=1.0 \
      'OTHER_CFLAGS=$(inherited) -Wno-unnecessary-virtual-specifier -Wno-nontrivial-memcall' \
      'OTHER_CPLUSPLUSFLAGS=$(inherited) -U_LIBCPP_ENABLE_ASSERTIONS -D_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_EXTENSIVE -Wno-unnecessary-virtual-specifier -Wno-nontrivial-memcall'
  )

  mkdir -p "$PREFIX/lib" "$PREFIX/include"
  rsync -a "$angle/ANGLE.xcarchive/Products/usr/local/lib/" "$PREFIX/lib/"
  rsync -a "$angle/include/" "$PREFIX/include/"
  for dylib in libEGL.dylib libGLESv2.dylib; do
    need_file "$PREFIX/lib/$dylib"
    install_name_tool -id "$PREFIX/lib/$dylib" "$PREFIX/lib/$dylib"
    codesign_file "$PREFIX/lib/$dylib"
  done

  write_pkgconfig "$PREFIX/lib/pkgconfig/angle.pc" angle "UTM WebKit ANGLE" "$ANGLE_COMMIT" '-lEGL -lGLESv2'
  write_pkgconfig "$PREFIX/lib/pkgconfig/egl.pc" egl "UTM WebKit ANGLE EGL" "$ANGLE_COMMIT" '-lEGL'
  write_pkgconfig "$PREFIX/lib/pkgconfig/glesv2.pc" glesv2 "UTM WebKit ANGLE GLESv2" "$ANGLE_COMMIT" '-lGLESv2'
}

build_epoxy() {
  local source="$DEPS_SRC/libepoxy"

  log "libepoxy"
  checkout_repo "$EPOXY_REPOSITORY" "$EPOXY_COMMIT" "$source"
  meson_install "$source" \
    -Degl=yes \
    -Dglx=no \
    -Dx11=false \
    -Dtests=false
  need_file "$PREFIX/lib/libepoxy.0.dylib"
  codesign_file "$PREFIX/lib/libepoxy.0.dylib"
  PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" pkg-config --variable=epoxy_has_egl epoxy | grep -qx 1
}

build_moltenvk() {
  local source="$DEPS_SRC/MoltenVK"
  local package dylib patch

  log "MoltenVK"
  checkout_repo "$MOLTENVK_REPOSITORY" "$MOLTENVK_COMMIT" "$source"
  for patch in "${MOLTENVK_PATCHES[@]}"; do
    apply_dependency_patch "$source" "$patch"
  done
  (
    cd "$source"
    clean_env ./fetchDependencies --macos -v
    rm -rf "Package/$CONFIGURATION"
    clean_env xcodebuild build \
      -project MoltenVKPackaging.xcodeproj \
      -scheme 'MoltenVK Package (macOS only)' \
      -configuration "$CONFIGURATION" \
      -sdk "$SDK" \
      -arch "$ARCH" \
      MACOSX_DEPLOYMENT_TARGET="$MINIMUM_MACOS" \
      CODE_SIGNING_ALLOWED=NO
  )

  package="$source/Package/$CONFIGURATION/MoltenVK"
  dylib="$package/dynamic/dylib/macOS/libMoltenVK.dylib"
  need_file "$dylib"
  install -m 755 "$dylib" "$PREFIX/lib/libMoltenVK.dylib"
  [ ! -d "$package/include" ] || rsync -a "$package/include/" "$PREFIX/include/"
  install_name_tool -id "$PREFIX/lib/libMoltenVK.dylib" "$PREFIX/lib/libMoltenVK.dylib"
  codesign_file "$PREFIX/lib/libMoltenVK.dylib"

  write_pkgconfig "$PREFIX/lib/pkgconfig/MoltenVK.pc" MoltenVK "UTM MoltenVK geometry shader build" "$MOLTENVK_VERSION" '-lMoltenVK'
  write_pkgconfig "$PREFIX/lib/pkgconfig/vulkan.pc" Vulkan "Vulkan loader backed by UTM MoltenVK" "$MOLTENVK_VERSION" '-lMoltenVK'
}

prepare_python() {
  [ -x "$DEPS_VENV/bin/python3" ] || python3 -m venv "$DEPS_VENV"
  "$DEPS_VENV/bin/python3" -m pip install \
    --disable-pip-version-check --quiet --upgrade pip pyyaml
}

virgl_patch_files() {
  local patch_name

  while IFS= read -r patch_name; do
    [ -n "$patch_name" ] || continue
    printf '%s\n' "$ROOT/patches/virglrenderer/$patch_name"
  done < "$VIRGL_PATCH_SERIES"
}

build_virglrenderer() {
  local source="$DEPS_SRC/virglrenderer"
  local epoxy_source="$DEPS_SRC/libepoxy"
  local patch
  local x86_root x86_prefix cross_file arm_server x86_server universal_server

  log "virglrenderer"
  checkout_repo "$VIRGL_REPOSITORY" "$VIRGL_COMMIT" "$source"

  apply_dependency_patch "$source" "$VIRGL_GENERATED_PATCH"

  while IFS= read -r patch; do
    apply_dependency_patch "$source" "$patch"
  done < <(virgl_patch_files)
  prepare_python
  (
    export PATH="$DEPS_VENV/bin:$PATH"
    meson_install "$source" \
      -Dvenus=true \
      -Dneptune=true \
      -Dvulkan-dload=false \
      -Dplatforms=egl \
      -Drender-server-mode=process \
      -Drender-server-worker=process \
      -Ddrm-renderers=[] \
      -Dtests=false \
      -Dcheck-gl-errors=false \
      -Dvideo=false \
      -Dtracing=none
  )

  need_file "$PREFIX/lib/libvirglrenderer.1.dylib"
  need_file "$PREFIX/libexec/virgl_render_server"

  # D3DMetal.framework is x86_64-only.  Build a minimal Neptune render-server
  # slice for Rosetta; the normal arm64 slice remains the full Venus/Neptune
  # build used by DXMT and Vulkan.
  x86_root="$(mktemp -d "${TMPDIR:-/tmp}/varmint-virgl-x86.XXXXXX")"
  x86_prefix="$x86_root/prefix"
  cross_file="$x86_root/macos-x86_64.ini"
  mkdir -p "$x86_prefix"

  write_macos_x86_64_cross_file "$cross_file"

  (
    export PATH="$DEPS_VENV/bin:$PATH"

    meson setup "$x86_root/epoxy-build" "$epoxy_source" \
      --cross-file "$cross_file" \
      --prefix "$x86_prefix" \
      --buildtype=release \
      -Degl=no \
      -Dglx=no \
      -Dx11=false \
      -Dtests=false
    meson compile -C "$x86_root/epoxy-build"
    meson install -C "$x86_root/epoxy-build"

    PKG_CONFIG_PATH="$x86_prefix/lib/pkgconfig" \
    PKG_CONFIG_LIBDIR="$x86_prefix/lib/pkgconfig:/usr/lib/pkgconfig" \
      meson setup "$x86_root/virgl-build" "$source" \
        --cross-file "$cross_file" \
        --prefix "$x86_prefix" \
        --buildtype=release \
        -Dvenus=false \
        -Dneptune=true \
        -Dvulkan-dload=false \
        -Dplatforms=[] \
        -Drender-server-mode=process \
        -Drender-server-worker=process \
        -Ddrm-renderers=[] \
        -Dtests=false \
        -Dcheck-gl-errors=false \
        -Dvideo=false \
        -Dtracing=none
    meson compile -C "$x86_root/virgl-build" virgl_render_server
  )

  arm_server="$PREFIX/libexec/virgl_render_server"
  x86_server="$x86_root/virgl-build/server/virgl_render_server"
  universal_server="$x86_root/virgl_render_server"

  need_file "$x86_server"
  lipo -create "$arm_server" "$x86_server" -output "$universal_server"
  mv "$universal_server" "$arm_server"
  rm -rf "$x86_root"

  lipo -archs "$arm_server" | grep -qw arm64
  lipo -archs "$arm_server" | grep -qw x86_64

  codesign_file "$PREFIX/lib/libvirglrenderer.1.dylib"
  codesign_file "$arm_server"
}

build_d3dmetal() {
  local source="$DEPS_SRC/d3dmetal-native"
  local x86_root x86_prefix cross_file built_library destination

  log "d3dmetal-native"
  checkout_repo "$D3DMETAL_REPOSITORY" "$D3DMETAL_COMMIT" "$source"

  for patch in "${D3DMETAL_PATCHES[@]}"; do
    apply_dependency_patch "$source" "$patch"
  done

  x86_root="$(mktemp -d "${TMPDIR:-/tmp}/varmint-d3dmetal-x86.XXXXXX")"
  x86_prefix="$x86_root/prefix"
  cross_file="$x86_root/macos-x86_64.ini"
  mkdir -p "$x86_prefix"

  write_macos_x86_64_cross_file "$cross_file"

  (
    export PATH="$DEPS_VENV/bin:$PATH"
    meson setup "$x86_root/build" "$source" \
      --cross-file "$cross_file" \
      --prefix "$x86_prefix" \
      --buildtype=release \
      -Dtests=disabled
    meson compile -C "$x86_root/build"
    meson install -C "$x86_root/build"
  )

  built_library="$x86_prefix/lib/libd3dmetal-native.dylib"
  destination="$PREFIX/lib/x86_64/libd3dmetal-native.dylib"

  need_file "$built_library"
  mkdir -p "$(dirname "$destination")"
  install -m 755 "$built_library" "$destination"
  rm -rf "$x86_root"

  lipo -archs "$destination" | tr ' ' '\n' | grep -qx x86_64 \
    || die "d3dmetal-native is missing x86_64 slice"
  codesign_file "$destination"
}

build_dxmt() {
  local source="$DEPS_SRC/dxmt"
  local llvm_path patch

  log "DXMT"
  checkout_repo "$DXMT_REPOSITORY" "$DXMT_COMMIT" "$source"
  git -C "$source" submodule update --init --recursive

  for patch in "${DXMT_PATCHES[@]}"; do
    apply_dependency_patch "$source" "$patch"
  done

  llvm_path="${DXMT_LLVM_PATH:-}"
  if [ -z "$llvm_path" ]; then
    need_cmd brew
    llvm_path="$(brew --prefix llvm@15)"
  fi
  [ -d "$llvm_path" ] || die "DXMT LLVM 15 not found: $llvm_path"

  meson_install "$source" \
    --buildtype=release \
    -Dnative_llvm_path="$llvm_path" \
    -Denable_tests=false \
    -Denable_nvapi=false \
    -Denable_d3d12=false

  need_file "$PREFIX/lib/libdxmt-native.dylib"
  codesign_file "$PREFIX/lib/libdxmt-native.dylib"
}

verify_dependency_prefix() {
  local dylib
  require_commands lipo pkg-config
  log "verify dependency prefix"
  for dylib in "${RUNTIME_DYLIBS[@]}"; do
    need_file "$PREFIX/lib/$dylib"
    lipo -archs "$PREFIX/lib/$dylib" | tr ' ' '\n' | grep -qx "$ARCH" \
      || die "$dylib does not contain $ARCH"
  done
  PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"     pkg-config --exists epoxy egl glesv2 vulkan dxmt-native
  need_file "$PREFIX/lib/x86_64/libd3dmetal-native.dylib"
  lipo -archs "$PREFIX/lib/x86_64/libd3dmetal-native.dylib" |
    tr ' ' '\n' | grep -qx x86_64 \
    || die "d3dmetal-native is missing x86_64 slice"

}

dependency_recipe_hash() {
  local patch

  cat \
    "$MANIFEST" \
    "${MOLTENVK_PATCHES[@]}" \
    "$VIRGL_GENERATED_PATCH" \
    "$VIRGL_PATCH_SERIES"

  while IFS= read -r patch; do
    cat "$patch"
  done < <(virgl_patch_files)

  cat \
    "${D3DMETAL_PATCHES[@]}" \
    "${DXMT_PATCHES[@]}" \
    "$COMMON_SCRIPT" \
    "$DEPENDENCIES_SCRIPT"

  printf '%s\n' "$SDK" "$ARCH" "$CONFIGURATION"
}

build_dependencies() {
  local stamp="$PREFIX/.varmint-dependencies"
  local expected_stamp patch

  for patch in \
    "${MOLTENVK_PATCHES[@]}" \
    "$VIRGL_GENERATED_PATCH" \
    "$VIRGL_PATCH_SERIES" \
    "${D3DMETAL_PATCHES[@]}" \
    "${DXMT_PATCHES[@]}"
  do
    need_file "$patch"
  done

  while IFS= read -r patch; do
    need_file "$patch"
  done < <(virgl_patch_files)
  require_commands git xcodebuild meson ninja pkg-config rsync \
    install_name_tool codesign lipo shasum

  expected_stamp="$(dependency_recipe_hash | shasum -a 256 | awk '{print $1}')"
  if [ "$FORCE_REBUILD" != 1 ] && [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$expected_stamp" ]; then
    if (verify_dependency_prefix); then
      log "dependencies are already current"
      return
    fi
    log "dependency prefix is incomplete; rebuilding"
  fi

  mkdir -p "$DEPS_SRC"
  begin_dependency_prefix_update
  build_angle
  build_epoxy
  build_moltenvk
  build_virglrenderer
  build_d3dmetal
  build_dxmt
  verify_dependency_prefix
  printf '%s\n' "$expected_stamp" > "$stamp"
  commit_dependency_prefix_update
  log "dependency prefix ready: $PREFIX"
}
