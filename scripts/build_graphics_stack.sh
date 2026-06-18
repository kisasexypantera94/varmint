#!/usr/bin/env bash
set -euo pipefail

# build_graphics_stack.sh
#
# Installs:
#   local/varmint/molten-vk-utm
#   local/varmint/virglrenderer-utm
#
# Assumes UTM MoltenVK is already built:
#   ~/dev/MoltenVK-utm/Package/Release/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib
#
# IMPORTANT:
#   Do NOT set HOMEBREW_NO_INSTALL_FROM_API=1.
#   It makes Homebrew clone homebrew/core; we do not need that for this local tap.

TAP_NAME="local/varmint"
MVK_SRC="${MVK_SRC:-$HOME/dev/MoltenVK-utm}"
MVK_PACKAGE="$MVK_SRC/Package/Release/MoltenVK"
MVK_DYLIB="$MVK_PACKAGE/dynamic/dylib/macOS/libMoltenVK.dylib"
MVK_VERSION="${MVK_VERSION:-1.4.2-utm-geometry}"

WORKDIR="${WORKDIR:-$HOME/dev/varmint-homebrew-build}"
TARBALL="$WORKDIR/molten-vk-utm-$MVK_VERSION.tar.gz"

echo "== Homebrew safety env =="
unset HOMEBREW_NO_INSTALL_FROM_API
export HOMEBREW_NO_AUTO_UPDATE=1

echo "== checking MoltenVK package =="
if [ ! -f "$MVK_DYLIB" ]; then
  echo "Missing: $MVK_DYLIB" >&2
  echo >&2
  echo "Build UTM MoltenVK first:" >&2
  echo "  cd \"$MVK_SRC\"" >&2
  echo "  make macos" >&2
  exit 1
fi

echo "== creating/finding local tap: $TAP_NAME =="
if ! brew --repo "$TAP_NAME" >/dev/null 2>&1; then
  brew tap-new "$TAP_NAME"
fi

TAP_REPO="$(brew --repo "$TAP_NAME")"
mkdir -p "$TAP_REPO/Formula"
mkdir -p "$WORKDIR"

echo "== removing partial homebrew-core clone if previous run got stuck =="
rm -rf /opt/homebrew/Library/Taps/homebrew/homebrew-core

echo "== packaging MoltenVK local tarball =="
rm -f "$TARBALL"
tar -C "$(dirname "$MVK_PACKAGE")" -czf "$TARBALL" "$(basename "$MVK_PACKAGE")"

MVK_SHA="$(shasum -a 256 "$TARBALL" | awk '{print $1}')"
MVK_URL="file://$TARBALL"

echo "MoltenVK package: $MVK_PACKAGE"
echo "Tarball:         $TARBALL"
echo "SHA256:          $MVK_SHA"

echo "== writing formula: molten-vk-utm =="
cat > "$TAP_REPO/Formula/molten-vk-utm.rb" <<RUBY
class MoltenVkUtm < Formula
  desc "UTM MoltenVK geometry-shader build for varmint"
  homepage "https://github.com/utmapp/UTM"
  url "$MVK_URL"
  sha256 "$MVK_SHA"
  version "$MVK_VERSION"
  license "Apache-2.0"

  keg_only "varmint uses this side-by-side with Homebrew molten-vk"

  def install
    # Homebrew may set buildpath to the archive root itself.
    # Support both layouts:
    #   buildpath/dynamic/...
    #   buildpath/MoltenVK/dynamic/...
    candidates = [
      buildpath,
      buildpath/"MoltenVK",
    ]

    mvk = candidates.find do |dir|
      (dir/"dynamic/dylib/macOS/libMoltenVK.dylib").exist?
    end

    odie "Could not find libMoltenVK.dylib under #{buildpath}" if mvk.nil?

    dylib = mvk/"dynamic/dylib/macOS/libMoltenVK.dylib"
    lib.install dylib

    if (mvk/"include").exist?
      include.install (mvk/"include").children
    end

    system "install_name_tool", "-id", "#{opt_lib}/libMoltenVK.dylib", "#{lib}/libMoltenVK.dylib"

    (lib/"pkgconfig").mkpath
    rm_f lib/"pkgconfig/MoltenVK.pc"
    (lib/"pkgconfig/MoltenVK.pc").write <<~EOS
      prefix=#{opt_prefix}
      exec_prefix=\${prefix}
      libdir=\${prefix}/lib
      includedir=\${prefix}/include

      Name: MoltenVK
      Description: UTM MoltenVK geometry-shader build
      Version: #{version}
      Libs: -L\${libdir} -lMoltenVK
      Cflags: -I\${includedir}
    EOS

    # Keep the ICD manifest inside the keg, not in HOMEBREW_PREFIX/etc.
    # Homebrew treats etc as persistent config and refuses to overwrite it
    # on reinstall, which breaks iterative local formula development.
    (share/"vulkan/icd.d").mkpath
    rm_f share/"vulkan/icd.d/MoltenVK_utm_icd.json"
    (share/"vulkan/icd.d/MoltenVK_utm_icd.json").write <<~JSON
      {
        "file_format_version": "1.0.0",
        "ICD": {
          "library_path": "#{opt_lib}/libMoltenVK.dylib",
          "api_version": "1.3.0"
        }
      }
    JSON
  end
end
RUBY

echo "== writing formula: virglrenderer-utm =="
cat > "$TAP_REPO/Formula/virglrenderer-utm.rb" <<'RUBY'
class VirglrendererUtm < Formula
  desc "libkrun-compatible virglrenderer linked against UTM MoltenVK"
  homepage "https://gitlab.freedesktop.org/virgl/virglrenderer"
  url "https://gitlab.freedesktop.org/slp/virglrenderer/-/archive/0.10.4e-krunkit/virglrenderer-0.10.4e-krunkit.tar.gz"
  sha256 "09d000623fbdb966cb604eb48c962a0815e8142383e6066d6494809335b76dbb"
  version "0.10.4e"
  license "MIT"

  keg_only "varmint links to this explicitly"

  depends_on "meson" => :build
  depends_on "ninja" => :build
  depends_on "pkg-config" => :build

  depends_on "libepoxy"
  depends_on "angle"
  depends_on "molten-vk-utm"

  def install
    mvk = Formula["molten-vk-utm"]

    ENV.prepend_path "PKG_CONFIG_PATH", mvk.opt_lib/"pkgconfig"
    ENV.append "LDFLAGS", "-Wl,-rpath,#{mvk.opt_lib}"

    args = %w[
      -Dvenus=true
      -Drender-server=false
    ]

    system "meson", "setup", "build", *args, *std_meson_args
    system "meson", "compile", "-C", "build", "--verbose"
    system "meson", "install", "-C", "build"
  end
end
RUBY

echo "== trusting local formulas if needed =="
brew trust --formula "$TAP_NAME/molten-vk-utm" || true
brew trust --formula "$TAP_NAME/virglrenderer-utm" || true

echo "== clean reinstall local formulas =="
# Remove dependent first, then dependency. These are local dev formulas only.
brew uninstall --force "$TAP_NAME/virglrenderer-utm" || true
brew uninstall --force "$TAP_NAME/molten-vk-utm" || true

# Extra cleanup for failed partial local installs of the same version.
rm -rf "$(brew --prefix)/Cellar/virglrenderer-utm"
rm -rf "$(brew --prefix)/Cellar/molten-vk-utm"
rm -f  "$(brew --prefix)/etc/vulkan/icd.d/MoltenVK_utm_icd.json"

brew install --build-from-source "$TAP_NAME/molten-vk-utm"
brew install --build-from-source "$TAP_NAME/virglrenderer-utm"


echo "== codesigning local graphics dylibs =="
MVK_PREFIX="$(brew --prefix "$TAP_NAME/molten-vk-utm")"
VIRGL_PREFIX="$(brew --prefix "$TAP_NAME/virglrenderer-utm")"

codesign --force --sign - --timestamp=none "$MVK_PREFIX/lib/libMoltenVK.dylib"
codesign --force --sign - --timestamp=none "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib"

codesign --verify --verbose=2 "$MVK_PREFIX/lib/libMoltenVK.dylib"
codesign --verify --verbose=2 "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib"

echo "== verification =="
MVK_PREFIX="$(brew --prefix "$TAP_NAME/molten-vk-utm")"
VIRGL_PREFIX="$(brew --prefix "$TAP_NAME/virglrenderer-utm")"

echo "molten-vk-utm prefix:     $MVK_PREFIX"
echo "virglrenderer-utm prefix: $VIRGL_PREFIX"

echo
echo "virglrenderer linked dylibs:"
otool -L "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib" | grep -Ei 'virgl|MoltenVK' || true

echo
echo "Expected important line:"
echo "  /opt/homebrew/opt/molten-vk-utm/lib/libMoltenVK.dylib"
