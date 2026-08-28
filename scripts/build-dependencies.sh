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

build_virglrenderer() {
  local source="$DEPS_SRC/virglrenderer"
  local patch

  log "virglrenderer"
  checkout_repo "$VIRGL_REPOSITORY" "$VIRGL_COMMIT" "$source"
  for patch in "${VIRGL_PATCHES[@]}"; do
    apply_dependency_patch "$source" "$patch"
  done
  prepare_python
  (
    export PATH="$DEPS_VENV/bin:$PATH"
    meson_install "$source" \
      -Dvenus=true \
      -Dneptune=true \
      -Dvulkan-dload=false \
      -Dplatforms=egl \
      -Drender-server-worker=thread \
      -Ddrm-renderers=[] \
      -Dtests=false \
      -Dcheck-gl-errors=false \
      -Dvideo=false \
      -Dtracing=none
  )
  need_file "$PREFIX/lib/libvirglrenderer.1.dylib"
  codesign_file "$PREFIX/lib/libvirglrenderer.1.dylib"
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

  meson_install "$source"     --buildtype=release     -Dnative_llvm_path="$llvm_path"     -Denable_tests=false     -Denable_nvapi=false     -Denable_d3d12=false

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
}

dependency_recipe_hash() {
  cat \
    "$MANIFEST" \
    "${MOLTENVK_PATCHES[@]}" \
    "${VIRGL_PATCHES[@]}" \
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
    "${VIRGL_PATCHES[@]}" \
    "${DXMT_PATCHES[@]}"
  do
    need_file "$patch"
  done
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
  build_dxmt
  verify_dependency_prefix
  printf '%s\n' "$expected_stamp" > "$stamp"
  commit_dependency_prefix_update
  log "dependency prefix ready: $PREFIX"
}
