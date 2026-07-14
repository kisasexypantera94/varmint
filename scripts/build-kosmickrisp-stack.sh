#!/usr/bin/env bash
set -euo pipefail

# Builds:
#   Mesa KosmicKrisp -> $HOME/dev/varmint-deps/kosmickrisp-utm
#   virglrenderer   -> $HOME/dev/varmint-deps/virglrenderer-utm
#
# Assumes existing:
#   ANGLE  -> $HOME/dev/varmint-deps/angle-utm
#   epoxy  -> $HOME/dev/varmint-deps/libepoxy-utm
#
# Important:
#   virglrenderer links to Vulkan loader, runtime selects KosmicKrisp via VK_ICD_FILENAMES.

VARMINT_DEPS="${VARMINT_DEPS:-$HOME/dev/varmint-deps}"
SRC_ROOT="${SRC_ROOT:-$VARMINT_DEPS/src}"

ANGLE_PREFIX="${ANGLE_PREFIX:-$VARMINT_DEPS/angle-utm}"
EPOXY_PREFIX="${EPOXY_PREFIX:-$VARMINT_DEPS/libepoxy-utm}"
KK_PREFIX="${KK_PREFIX:-$VARMINT_DEPS/kosmickrisp-utm}"
VIRGL_PREFIX="${VIRGL_PREFIX:-$VARMINT_DEPS/virglrenderer-utm}"

MESA_REPO="${MESA_REPO:-https://gitlab.freedesktop.org/mesa/mesa.git}"
MESA_REF="${MESA_REF:-main}"
MESA_SRC="${MESA_SRC:-$SRC_ROOT/mesa-kosmickrisp}"

VIRGL_REPO="${VIRGL_REPO:-https://github.com/utmapp/virglrenderer.git}"
VIRGL_COMMIT="${VIRGL_COMMIT:-d48a2d0d9a722fffd3f92c83e71d9426a4892a66}"
VIRGL_SRC="${VIRGL_SRC:-$SRC_ROOT/virglrenderer-utm}"

VULKAN_LOADER_PREFIX="${VULKAN_LOADER_PREFIX:-$(brew --prefix vulkan-loader)}"
LLVM_PREFIX="${LLVM_PREFIX:-$(brew --prefix llvm)}"
LIBCLC_PREFIX="${LIBCLC_PREFIX:-$(brew --prefix libclc)}"
SPIRV_LLVM_PREFIX="${SPIRV_LLVM_PREFIX:-$(brew --prefix spirv-llvm-translator)}"

CLEAN="${CLEAN:-1}"
SKIP_KK="${SKIP_KK:-0}"
SKIP_VIRGL="${SKIP_VIRGL:-0}"

log() {
  printf '\n== %s ==\n' "$*"
}

die() {
  echo "error: $*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

need_file() {
  [ -f "$1" ] || die "missing file: $1"
}

codesign_if_exists() {
  local f="$1"
  [ -e "$f" ] || return 0
  codesign --force --sign - --timestamp=none "$f" || true
  codesign --verify --verbose=2 "$f" || true
}

checkout_repo() {
  local repo="$1"
  local ref="$2"
  local dir="$3"

  mkdir -p "$(dirname "$dir")"

  if [ ! -d "$dir/.git" ]; then
    rm -rf "$dir"
    git clone "$repo" "$dir"
  fi

  cd "$dir"
  git fetch origin '+refs/heads/*:refs/remotes/origin/*' '+refs/tags/*:refs/tags/*'

  if git rev-parse --verify --quiet "origin/$ref" >/dev/null; then
    git checkout --detach "origin/$ref"
  else
    git checkout --detach "$ref"
  fi

  git rev-parse HEAD
}

build_kosmickrisp() {
  log "Mesa/KosmicKrisp checkout"
  checkout_repo "$MESA_REPO" "$MESA_REF" "$MESA_SRC" >/dev/null

  log "Mesa patch: skip LLVMSPIRVLib block"
  cd "$MESA_SRC"
  python3 - <<'PY_MESA_PATCH'
from pathlib import Path

p = Path("meson.build")
lines = p.read_text().splitlines()

hit = None
for i, line in enumerate(lines):
    if "LLVMSPIRVLib" in line:
        hit = i
        break

if hit is None:
    raise SystemExit("cannot find LLVMSPIRVLib in meson.build")

target = None
for j in range(hit - 1, max(-1, hit - 120), -1):
    stripped = lines[j].strip()
    if stripped.startswith("if ") and (
        "opencl" in stripped
        or "spirv" in stripped
        or "clc" in stripped
        or target is None
    ):
        target = j
        if "opencl" in stripped or "spirv" in stripped or "clc" in stripped:
            break

if target is None:
    raise SystemExit("cannot find enclosing if before LLVMSPIRVLib")

if "varmint: skip LLVMSPIRVLib" not in "\n".join(lines[max(0, target-2):target+2]):
    indent = lines[target][:len(lines[target]) - len(lines[target].lstrip())]
    old = lines[target]
    lines[target] = indent + "# varmint: skip LLVMSPIRVLib / OpenCL-SPIR-V path for KosmicKrisp-only build"
    lines.insert(target + 1, indent + "if false")
    print("patched line", target + 1, ":", old)
else:
    print("already patched")

p.write_text("\n".join(lines) + "\n")
PY_MESA_PATCH

  log "Mesa/KosmicKrisp build"
  cd "$MESA_SRC"

  if [ "$CLEAN" = "1" ]; then
    rm -rf build-kosmickrisp
  fi

  python3 -m venv "$VARMINT_DEPS/venv-mesa"
  # shellcheck disable=SC1091
  source "$VARMINT_DEPS/venv-mesa/bin/activate"

  python3 -m pip install -U pip
  python3 -m pip install -U mako pyyaml packaging



  # Avoid GStreamer.framework pkg-config zlib poisoning Mesa link flags,
  # but keep Homebrew LLVM/libclc pkg-config dirs visible.
  export PKG_CONFIG_PATH="$(printf '%s' "${PKG_CONFIG_PATH:-}" | tr ':' '\n' | grep -v '/Library/Frameworks/GStreamer.framework' | paste -sd ':' -)"
  export PKG_CONFIG_LIBDIR="$SPIRV_LLVM_PREFIX/lib/pkgconfig:$SPIRV_LLVM_PREFIX/share/pkgconfig:$LIBCLC_PREFIX/share/pkgconfig:$LIBCLC_PREFIX/lib/pkgconfig:$LLVM_PREFIX/lib/pkgconfig:/opt/homebrew/lib/pkgconfig:/opt/homebrew/share/pkgconfig"
  export PKG_CONFIG_PATH="$SPIRV_LLVM_PREFIX/lib/pkgconfig:$SPIRV_LLVM_PREFIX/share/pkgconfig:$LIBCLC_PREFIX/share/pkgconfig:$LIBCLC_PREFIX/lib/pkgconfig:$LLVM_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

  export LLVM_CONFIG="$LLVM_PREFIX/bin/llvm-config"
  export PATH="$LLVM_PREFIX/bin:$PATH"
  export PKG_CONFIG_PATH="$LLVM_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  export LDFLAGS="-L$SPIRV_LLVM_PREFIX/lib -L$LLVM_PREFIX/lib -lLLVMSPIRVLib ${LDFLAGS:-}"
  export CPPFLAGS="-I$SPIRV_LLVM_PREFIX/include -I$LLVM_PREFIX/include ${CPPFLAGS:-}"

  meson setup build-kosmickrisp \
    --prefix "$KK_PREFIX" \
    --buildtype=release \
    -Dplatforms=macos \
    -Dvulkan-drivers=kosmickrisp \
    -Dgallium-drivers= \
    -Dopengl=false \
    -Dmicrosoft-clc=disabled \
    -Dspirv-to-dxil=false \
    -Dgallium-rusticl=false \
    -Dzstd=disabled \

  meson compile -C build-kosmickrisp --verbose

  rm -rf "$KK_PREFIX"
  meson install -C build-kosmickrisp

  log "Mesa/KosmicKrisp verify"
  find "$KK_PREFIX" -maxdepth 5 -type f | grep -Ei 'kosmic|vulkan|icd|\.dylib' || true

  local icd
  icd="$(find "$KK_PREFIX/share/vulkan/icd.d" -type f -name '*.json' 2>/dev/null | head -n 1 || true)"
  [ -n "$icd" ] || die "KosmicKrisp ICD json not found in $KK_PREFIX/share/vulkan/icd.d"

  find "$KK_PREFIX/lib" -type f -name '*.dylib' -print0 2>/dev/null \
    | while IFS= read -r -d '' f; do codesign_if_exists "$f"; done

  echo "$icd" > "$KK_PREFIX/share/varmint-kosmickrisp-icd.txt"
}

build_virglrenderer_against_kosmickrisp_loader() {
  log "virglrenderer checkout"
  checkout_repo "$VIRGL_REPO" "$VIRGL_COMMIT" "$VIRGL_SRC" >/dev/null

  log "virglrenderer build against Vulkan loader"
  cd "$VIRGL_SRC"

  if [ "$CLEAN" = "1" ]; then
    rm -rf build
  fi

  python3 - <<'PY'
import importlib.util
import subprocess
import sys

if importlib.util.find_spec("yaml") is None:
    subprocess.check_call([
        sys.executable,
        "-m",
        "pip",
        "install",
        "--break-system-packages",
        "pyyaml",
    ])
PY

  export PKG_CONFIG_PATH="$EPOXY_PREFIX/lib/pkgconfig:$ANGLE_PREFIX/lib/pkgconfig:$VULKAN_LOADER_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  export CFLAGS="-I$ANGLE_PREFIX/include -I$EPOXY_PREFIX/include -I$VULKAN_LOADER_PREFIX/include ${CFLAGS:-}"
  export CPPFLAGS="-I$ANGLE_PREFIX/include -I$EPOXY_PREFIX/include -I$VULKAN_LOADER_PREFIX/include ${CPPFLAGS:-}"
  export LDFLAGS="-L$ANGLE_PREFIX/lib -L$EPOXY_PREFIX/lib -L$VULKAN_LOADER_PREFIX/lib -Wl,-rpath,$ANGLE_PREFIX/lib -Wl,-rpath,$EPOXY_PREFIX/lib -Wl,-rpath,$VULKAN_LOADER_PREFIX/lib ${LDFLAGS:-}"

  meson setup build \
    --prefix "$VIRGL_PREFIX" \
    -Dvenus=true \
    -Dvulkan-dload=false \
    -Dplatforms=egl \
    -Drender-server-worker=thread \
    -Ddrm-renderers=[] \
    -Dtests=false \
    -Dcheck-gl-errors=false \
    -Dvideo=false \
    -Dtracing=none

  meson compile -C build --verbose

  rm -rf "$VIRGL_PREFIX"
  meson install -C build

  mkdir -p "$VIRGL_PREFIX/share/varmint"
  git rev-parse HEAD > "$VIRGL_PREFIX/share/varmint/virglrenderer-commit.txt"

  local dylib="$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib"
  need_file "$dylib"

  for rpath in "$ANGLE_PREFIX/lib" "$EPOXY_PREFIX/lib" "$VULKAN_LOADER_PREFIX/lib" "$KK_PREFIX/lib"; do
    if [ -d "$rpath" ] && ! otool -l "$dylib" | grep -Fq "path $rpath"; then
      install_name_tool -add_rpath "$rpath" "$dylib" || true
    fi
  done

  codesign_if_exists "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib"
  codesign_if_exists "$VIRGL_PREFIX/libexec/virgl_render_server"
  codesign_if_exists "$VIRGL_PREFIX/bin/virgl_test_server"
}

write_env() {
  log "write env helper"

  local icd
  icd="$(cat "$KK_PREFIX/share/varmint-kosmickrisp-icd.txt")"

  cat > "$VARMINT_DEPS/env-kosmickrisp.sh" <<EOF2
export VIRGL_UTM="$VIRGL_PREFIX"
export EPOXY_UTM="$EPOXY_PREFIX"
export ANGLE_UTM="$ANGLE_PREFIX"
export KK_UTM="$KK_PREFIX"
export VULKAN_LOADER="$VULKAN_LOADER_PREFIX"
export KK_ICD="$icd"

export VK_ICD_FILENAMES="\$KK_ICD"
export PKG_CONFIG_PATH="\$VIRGL_UTM/lib/pkgconfig:\$EPOXY_UTM/lib/pkgconfig:\$ANGLE_UTM/lib/pkgconfig:\$VULKAN_LOADER/lib/pkgconfig:\${PKG_CONFIG_PATH:-}"
export DYLD_LIBRARY_PATH="\$VIRGL_UTM/lib:\$EPOXY_UTM/lib:\$ANGLE_UTM/lib:\$VULKAN_LOADER/lib:\$KK_UTM/lib:\${DYLD_LIBRARY_PATH:-}"
export DYLD_FRAMEWORK_PATH="\$ANGLE_UTM/lib:\${DYLD_FRAMEWORK_PATH:-}"
export RUSTFLAGS="-L \$VIRGL_UTM/lib -L \$EPOXY_UTM/lib -L \$ANGLE_UTM/lib -L \$VULKAN_LOADER/lib -L \$KK_UTM/lib -L /opt/homebrew/lib \${RUSTFLAGS:-}"
EOF2

  echo "$VARMINT_DEPS/env-kosmickrisp.sh"
}

verify() {
  log "verify"

  echo "ANGLE_PREFIX=$ANGLE_PREFIX"
  echo "EPOXY_PREFIX=$EPOXY_PREFIX"
  echo "KK_PREFIX=$KK_PREFIX"
  echo "VULKAN_LOADER_PREFIX=$VULKAN_LOADER_PREFIX"
  echo "VIRGL_PREFIX=$VIRGL_PREFIX"
  echo "KK_ICD=$(cat "$KK_PREFIX/share/varmint-kosmickrisp-icd.txt")"

  echo
  echo "virglrenderer linked dylibs:"
  otool -L "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib" \
    | grep -Ei 'virgl|MoltenVK|epoxy|EGL|GLES|vulkan|kosmic|mesa|System' || true

  echo
  echo "Expected:"
  echo "  - should see libvulkan"
  echo "  - should NOT see libMoltenVK"
}

main() {
  need_cmd git
  need_cmd meson
  need_cmd ninja
  need_cmd pkg-config
  need_cmd brew
  need_cmd codesign
  need_cmd install_name_tool
  need_cmd otool
  need_cmd python3

  need_file "$ANGLE_PREFIX/lib/libEGL.dylib"
  need_file "$ANGLE_PREFIX/lib/libGLESv2.dylib"
  need_file "$EPOXY_PREFIX/lib/libepoxy.0.dylib"
  need_file "$VULKAN_LOADER_PREFIX/lib/pkgconfig/vulkan.pc"

  mkdir -p "$SRC_ROOT" "$VARMINT_DEPS"

  log "inputs"
  echo "VARMINT_DEPS=$VARMINT_DEPS"
  echo "MESA_SRC=$MESA_SRC"
  echo "MESA_REF=$MESA_REF"
  echo "KK_PREFIX=$KK_PREFIX"
  echo "VULKAN_LOADER_PREFIX=$VULKAN_LOADER_PREFIX"
  echo "VIRGL_SRC=$VIRGL_SRC"
  echo "VIRGL_PREFIX=$VIRGL_PREFIX"

  [ "$SKIP_KK" = "1" ] || build_kosmickrisp
  [ "$SKIP_VIRGL" = "1" ] || build_virglrenderer_against_kosmickrisp_loader

  write_env
  verify

  log "done"
}

main "$@"
