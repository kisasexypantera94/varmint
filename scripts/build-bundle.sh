build_varmint() {
  local cargo_target_dir="${CARGO_TARGET_DIR:-$BUILD_ROOT/cargo}"
  local cargo_args=(build --release)
  local dylib

  require_commands cargo pkg-config codesign
  need_file "$ENTITLEMENTS"
  for dylib in "${RUNTIME_DYLIBS[@]}"; do
    need_file "$PREFIX/lib/$dylib"
  done
  [ ! -f "$ROOT/Cargo.lock" ] || cargo_args+=(--locked)

  log "build varmint"
  (
    cd "$ROOT"
    MACOSX_DEPLOYMENT_TARGET="$MINIMUM_MACOS" \
    PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}" \
    RUSTFLAGS="-L native=$PREFIX/lib ${RUSTFLAGS:-}" \
    CARGO_TARGET_DIR="$cargo_target_dir" \
      cargo "${cargo_args[@]}"
  )

  mkdir -p "$(dirname "$BINARY")"
  install -m 755 "$cargo_target_dir/release/varmint" "$BINARY"
  codesign --force --sign - --timestamp=none --entitlements "$ENTITLEMENTS" "$BINARY"
  codesign --verify --strict --verbose=2 "$BINARY"
}

build_icon() {
  log "build app icon"
  local runtime="$BUILD_ROOT/runtime"
  local source="$runtime/Varmint.icon"
  local output="$runtime/icon-assets"
  local plist="$runtime/icon-partial-info.plist"

  need_cmd xcrun
  [ -d "$ICON_SOURCE" ] || die "missing Icon Composer asset: $ICON_SOURCE"
  need_file "$ICON_SOURCE/icon.json"

  rm -rf "$source" "$output"
  rm -f "$ICON" "$plist"
  mkdir -p "$output" "$(dirname "$ICON")"
  cp -R "$ICON_SOURCE" "$source"
  xcrun actool "$source" \
    --compile "$output" \
    --output-format human-readable-text \
    --notices --warnings --errors \
    --output-partial-info-plist "$plist" \
    --app-icon Varmint \
    --include-all-app-icons \
    --enable-on-demand-resources NO \
    --development-region en \
    --target-device mac \
    --minimum-deployment-target "$MINIMUM_MACOS" \
    --platform macosx

  need_file "$output/Assets.car"
  install -m 644 "$output/Assets.car" "$ICON"
  xcrun --sdk macosx assetutil --info "$ICON" >/dev/null
  rm -rf "$source" "$output"
  rm -f "$plist"
}

find_vmnet_helper() {
  local prefix helper

  if [ -n "$VMNET_HELPER" ]; then
    need_file "$VMNET_HELPER"
    printf '%s\n' "$VMNET_HELPER"
    return
  fi

  need_cmd brew
  prefix="$(brew --prefix vmnet-helper 2>/dev/null)" \
    || die "vmnet-helper is not installed; run: brew tap nirs/vmnet-helper && brew install vmnet-helper"
  helper="$prefix/libexec/vmnet-helper"
  need_file "$helper"
  printf '%s\n' "$helper"
}

assemble_app() {
  log "assemble app"
  local contents="$APP_BUNDLE/Contents"
  local helper file

  require_commands plutil
  for file in "$BINARY" "$KERNEL" "$INITRD" "$BASE_IMAGE" "$ICON" "$MANIFEST"; do
    need_file "$file"
  done
  helper="$(find_vmnet_helper)"

  rm -rf "$APP_BUNDLE"
  mkdir -p \
    "$contents/MacOS" \
    "$contents/Helpers" \
    "$contents/Frameworks" \
    "$contents/Resources/kernel" \
    "$contents/Resources/runtime"
  install -m 755 "$BINARY" "$contents/MacOS/varmint"
  install -m 755 "$helper" "$contents/Helpers/vmnet-helper"
  install -m 644 "$KERNEL" "$contents/Resources/kernel/Image"
  install -m 644 "$INITRD" "$contents/Resources/kernel/initrd"
  install -m 644 "$BASE_IMAGE" "$contents/Resources/runtime/base.raw.zst"
  install -m 644 "$ICON" "$contents/Resources/Assets.car"
  install -m 644 "$MANIFEST" "$contents/Resources/runtime-manifest.toml"

  cat > "$contents/Info.plist" <<EOF_PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Varmint</string>
    <key>CFBundleExecutable</key>
    <string>varmint</string>
    <key>CFBundleIdentifier</key>
    <string>dev.varmint.Varmint</string>
    <key>CFBundleIconName</key>
    <string>Varmint</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Varmint</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$APP_VERSION</string>
    <key>CFBundleVersion</key>
    <string>$APP_VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>$MINIMUM_MACOS</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF_PLIST
  printf 'APPL????' > "$contents/PkgInfo"
  plutil -lint "$contents/Info.plist"
}

is_system_dependency() {
  case "$1" in
    /usr/lib/*|/System/Library/*) return 0 ;;
    *) return 1 ;;
  esac
}

list_rpaths() {
  otool -l "$1" | awk '
    $1 == "cmd" && $2 == "LC_RPATH" { in_rpath = 1; next }
    in_rpath && $1 == "path" {
      line = $0
      sub(/^[[:space:]]*path[[:space:]]+/, "", line)
      sub(/[[:space:]]+\(offset [0-9]+\)$/, "", line)
      print line
      in_rpath = 0
    }
  '
}

normalize_rpath() {
  local file="$1"
  local wanted="$2"
  local rpath found=0

  while IFS= read -r rpath; do
    [ -n "$rpath" ] || continue
    if [ "$rpath" = "$wanted" ]; then
      found=1
    else
      install_name_tool -delete_rpath "$rpath" "$file"
    fi
  done < <(list_rpaths "$file")
  [ "$found" = 1 ] || install_name_tool -add_rpath "$wanted" "$file"
}

resolve_dependency() {
  local dependency="$1"
  local basename="${dependency##*/}"

  case "$dependency" in
    "$PREFIX"/*) printf '%s\n' "$dependency" ;;
    @rpath/*|@loader_path/*|@executable_path/*) printf '%s\n' "$PREFIX/lib/$basename" ;;
    /*)
      [ -f "$PREFIX/lib/$basename" ] \
        || die "non-system dependency is outside build prefix: $dependency"
      printf '%s\n' "$PREFIX/lib/$basename"
      ;;
    *) printf '%s\n' "$PREFIX/lib/$basename" ;;
  esac
}

copy_runtime_library() {
  local source="$1"
  local destination="$APP_BUNDLE/Contents/Frameworks/$(basename "$source")"

  need_file "$source"
  [ -f "$destination" ] && return
  cp -L "$source" "$destination"
  chmod u+w "$destination"
  MACHO_QUEUE+=("$destination")
}

process_macho() {
  local file="$1"
  local executable="$2"
  local dependency source basename

  [ "$file" = "$executable" ] \
    || install_name_tool -id "@rpath/$(basename "$file")" "$file"

  while IFS= read -r dependency; do
    [ -n "$dependency" ] || continue
    is_system_dependency "$dependency" && continue
    source="$(resolve_dependency "$dependency")"
    basename="$(basename "$source")"
    copy_runtime_library "$source"
    install_name_tool -change "$dependency" "@rpath/$basename" "$file"
  done < <(otool -L "$file" | tail -n +2 | sed -E 's/^[[:space:]]*//; s/[[:space:]]+\(compatibility version.*$//')

  if [ "$file" = "$executable" ]; then
    normalize_rpath "$file" '@executable_path/../Frameworks'
  else
    normalize_rpath "$file" '@loader_path'
  fi
}

write_framework_plist() {
  local stem="$1"
  local plist="$2"

  cat > "$plist" <<EOF_PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>$stem</string>
  <key>CFBundleIdentifier</key><string>dev.varmint.angle.$stem</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$stem</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
</dict>
</plist>
EOF_PLIST
}

create_angle_frameworks() {
  local frameworks="$APP_BUNDLE/Contents/Frameworks"
  local stem loose framework binary resources

  for stem in EGL GLESv2; do
    loose="$frameworks/lib${stem}.dylib"
    framework="$frameworks/${stem}.framework"
    binary="$framework/Versions/A/$stem"
    resources="$framework/Versions/A/Resources"
    need_file "$loose"

    mkdir -p "$resources"
    mv "$loose" "$binary"
    write_framework_plist "$stem" "$resources/Info.plist"
    ln -s A "$framework/Versions/Current"
    ln -s "Versions/Current/$stem" "$framework/$stem"
    ln -s Versions/Current/Resources "$framework/Resources"
    ln -s "${stem}.framework/Versions/Current/$stem" "$loose"
  done
}

fix_rpaths() {
  log "bundle runtime libraries"
  local executable="$APP_BUNDLE/Contents/MacOS/varmint"
  local dylib index=0

  require_commands otool install_name_tool
  need_file "$executable"
  MACHO_QUEUE=("$executable")
  for dylib in "${RUNTIME_DYLIBS[@]}"; do
    copy_runtime_library "$PREFIX/lib/$dylib"
  done

  while [ "$index" -lt "${#MACHO_QUEUE[@]}" ]; do
    process_macho "${MACHO_QUEUE[$index]}" "$executable"
    index=$((index + 1))
  done
  create_angle_frameworks
  otool -L "$executable"
}

sign_app() {
  log "sign app"
  local frameworks="$APP_BUNDLE/Contents/Frameworks"
  local stem dylib

  need_cmd codesign
  need_file "$ENTITLEMENTS"
  need_file "$VMNET_HELPER_ENTITLEMENTS"
  codesign --force --sign - --timestamp=none \
    --entitlements "$VMNET_HELPER_ENTITLEMENTS" "$APP_BUNDLE/Contents/Helpers/vmnet-helper"
  while IFS= read -r -d '' dylib; do
    codesign_file "$dylib"
  done < <(find "$frameworks" -maxdepth 1 -type f -name '*.dylib' -print0)
  for stem in EGL GLESv2; do
    need_file "$frameworks/${stem}.framework/Versions/A/$stem"
    need_file "$frameworks/${stem}.framework/Versions/A/Resources/Info.plist"
    codesign_file "$frameworks/${stem}.framework"
  done
  codesign --force --sign - --timestamp=none \
    --entitlements "$ENTITLEMENTS" "$APP_BUNDLE"
}

verify_bundle_dependency() {
  local owner="$1"
  local dependency="$2"
  local executable="$3"
  local frameworks="$4"
  local resolved

  is_system_dependency "$dependency" && return
  case "$dependency" in
    @rpath/*) resolved="$frameworks/${dependency##*/}" ;;
    @loader_path/*) resolved="$(dirname "$owner")/${dependency#@loader_path/}" ;;
    @executable_path/*) resolved="$(dirname "$executable")/${dependency#@executable_path/}" ;;
    *) die "$owner contains forbidden dependency: $dependency" ;;
  esac
  [ -f "$resolved" ] || die "$owner references missing dependency: $dependency"
}

verify_bundle_macho() {
  local file="$1"
  local executable="$2"
  local frameworks="$3"
  local dependency rpath id expected_id

  lipo -archs "$file" | tr ' ' '\n' | grep -qx "$ARCH" \
    || die "$file does not contain $ARCH"
  while IFS= read -r dependency; do
    [ -n "$dependency" ] && verify_bundle_dependency \
      "$file" "$dependency" "$executable" "$frameworks"
  done < <(otool -L "$file" | tail -n +2 | sed -E 's/^[[:space:]]*//; s/[[:space:]]+\(compatibility version.*$//')
  while IFS= read -r rpath; do
    case "$rpath" in
      '@executable_path/../Frameworks'|'@loader_path') ;;
      *) die "$file contains forbidden rpath: $rpath" ;;
    esac
  done < <(list_rpaths "$file")

  [ "$file" = "$executable" ] && return
  case "$file" in
    "$frameworks/EGL.framework/Versions/A/EGL") expected_id='@rpath/libEGL.dylib' ;;
    "$frameworks/GLESv2.framework/Versions/A/GLESv2") expected_id='@rpath/libGLESv2.dylib' ;;
    *) expected_id="@rpath/$(basename "$file")" ;;
  esac
  id="$(otool -D "$file" | sed -n '2p')"
  [ "$id" = "$expected_id" ] || die "$file has invalid install name: $id"
}

verify_app() {
  local contents="$APP_BUNDLE/Contents"
  local executable="$contents/MacOS/varmint"
  local helper="$contents/Helpers/vmnet-helper"
  local frameworks="$contents/Frameworks"
  local resources="$contents/Resources"
  local bundled_manifest="$resources/runtime-manifest.toml"
  local stem dylib file signed_entitlements

  require_commands cmp plutil otool lipo codesign xcrun
  for file in \
    "$contents/Info.plist" \
    "$executable" \
    "$helper" \
    "$resources/kernel/Image" \
    "$resources/kernel/initrd" \
    "$resources/runtime/base.raw.zst" \
    "$resources/Assets.car" \
    "$bundled_manifest"; do
    need_file "$file"
  done
  cmp -s "$MANIFEST" "$bundled_manifest" || die "bundled runtime manifest differs from source"
  for stem in EGL GLESv2; do
    need_file "$frameworks/${stem}.framework/Versions/A/$stem"
    need_file "$frameworks/${stem}.framework/Versions/A/Resources/Info.plist"
  done

  log "verify app"
  plutil -lint "$contents/Info.plist" >/dev/null
  ! plutil -extract CFBundleIconFile raw -o - "$contents/Info.plist" >/dev/null 2>&1 \
    || die "Info.plist still contains legacy CFBundleIconFile"
  [ "$(plutil -extract CFBundleExecutable raw -o - "$contents/Info.plist")" = varmint ] \
    || die "Info.plist does not reference varmint"
  [ "$(plutil -extract CFBundleIconName raw -o - "$contents/Info.plist")" = Varmint ] \
    || die "Info.plist does not reference the Varmint asset icon"
  [ "$(plutil -extract LSMinimumSystemVersion raw -o - "$contents/Info.plist")" = "$MINIMUM_MACOS" ] \
    || die "Info.plist minimum macOS does not match runtime manifest"
  for file in "$resources/kernel/Image" "$resources/kernel/initrd" "$resources/runtime/base.raw.zst" "$resources/Assets.car"; do
    [ -s "$file" ] || die "bundled resource is empty: $file"
  done
  xcrun --sdk macosx assetutil --info "$resources/Assets.car" >/dev/null

  for stem in EGL GLESv2; do
    plutil -lint "$frameworks/${stem}.framework/Versions/A/Resources/Info.plist" >/dev/null
    [ "$(readlink "$frameworks/lib${stem}.dylib")" = "${stem}.framework/Versions/Current/$stem" ] \
      || die "lib${stem}.dylib is not linked to ${stem}.framework"
  done

  python3 - "$APP_BUNDLE" <<'PY'
from pathlib import Path
import sys

bundle = Path(sys.argv[1]).resolve()
for link in Path(sys.argv[1]).rglob("*"):
    if not link.is_symlink():
        continue
    try:
        target = link.resolve(strict=True)
    except FileNotFoundError:
        raise SystemExit(f"broken bundle symlink: {link}")
    if target != bundle and bundle not in target.parents:
        raise SystemExit(f"bundle symlink escapes app: {link} -> {target}")
PY

  verify_bundle_macho "$executable" "$executable" "$frameworks"
  while IFS= read -r -d '' dylib; do
    verify_bundle_macho "$dylib" "$executable" "$frameworks"
  done < <(find "$frameworks" -maxdepth 1 -type f -name '*.dylib' -print0)
  for stem in EGL GLESv2; do
    verify_bundle_macho "$frameworks/${stem}.framework/Versions/A/$stem" "$executable" "$frameworks"
  done
  for dylib in "${RUNTIME_DYLIBS[@]}"; do
    need_file "$frameworks/$dylib"
  done

  for stem in EGL GLESv2; do
    codesign --verify --strict --verbose=2 "$frameworks/${stem}.framework"
  done
  codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
  signed_entitlements="$(codesign -d --entitlements :- "$helper" 2>&1)"
  printf '%s\n' "$signed_entitlements" | grep -q '<key>com.apple.security.virtualization</key>' \
    || die "vmnet-helper is missing the Virtualization entitlement"
  signed_entitlements="$(codesign -d --entitlements :- "$executable" 2>&1)"
  printf '%s\n' "$signed_entitlements" | grep -q '<key>com.apple.security.hypervisor</key>' \
    || die "main executable is missing the Hypervisor entitlement"
  ! printf '%s\n' "$signed_entitlements" | grep -q '<key>com.apple.vm.networking</key>' \
    || die "ad-hoc bundle contains restricted com.apple.vm.networking entitlement"
}

build_app_bundle() {
  build_varmint
  build_icon
  assemble_app
  fix_rpaths
  sign_app
  verify_app
}
