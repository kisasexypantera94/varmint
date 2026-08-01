log() {
  printf '\n== %s ==\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

need_file() {
  [ -f "$1" ] || die "missing file: $1"
}

require_commands() {
  local command
  for command in "$@"; do
    need_cmd "$command"
  done
}

load_metadata() {
  local metadata
  need_cmd python3
  need_file "$ROOT/Cargo.toml"
  metadata="$(python3 - "$MANIFEST" "$ROOT/Cargo.toml" <<'PY'
import shlex
import sys
import tomllib

with open(sys.argv[1], "rb") as f:
    manifest = tomllib.load(f)
with open(sys.argv[2], "rb") as f:
    cargo = tomllib.load(f)

if manifest.get("schema_version") != 1:
    raise SystemExit("unsupported runtime manifest schema")
runtime = manifest["runtime"]
architectures = runtime["architectures"]
if len(architectures) != 1:
    raise SystemExit("runtime manifest must contain exactly one architecture")
dependencies = manifest["dependencies"]
values = {
    "ARCH": architectures[0],
    "APP_VERSION": cargo["package"]["version"],
    "MINIMUM_MACOS": runtime["minimum_macos"],
    "ANGLE_REPOSITORY": dependencies["angle"]["repository"],
    "ANGLE_COMMIT": dependencies["angle"]["commit"],
    "EPOXY_REPOSITORY": dependencies["epoxy"]["repository"],
    "EPOXY_COMMIT": dependencies["epoxy"]["commit"],
    "MOLTENVK_REPOSITORY": dependencies["moltenvk"]["repository"],
    "MOLTENVK_COMMIT": dependencies["moltenvk"]["commit"],
    "MOLTENVK_VERSION": dependencies["moltenvk"]["version"],
    "VIRGL_REPOSITORY": dependencies["virglrenderer"]["repository"],
    "VIRGL_COMMIT": dependencies["virglrenderer"]["commit"],
}
for key, value in values.items():
    print(f"{key}={shlex.quote(str(value))}")
print("RUNTIME_DYLIBS=(%s)" % " ".join(shlex.quote(item) for item in runtime["dylibs"]))
PY
)" || die "failed to load build metadata"
  eval "$metadata"
}

checkout_repo() {
  local repository="$1"
  local commit="$2"
  local destination="$3"

  mkdir -p "$(dirname "$destination")"
  if [ ! -d "$destination/.git" ]; then
    rm -rf "$destination"
    git clone "$repository" "$destination"
  fi

  git -C "$destination" reset --hard HEAD >/dev/null
  git -C "$destination" fetch origin \
    '+refs/heads/*:refs/remotes/origin/*' \
    '+refs/tags/*:refs/tags/*'
  if ! git -C "$destination" cat-file -e "$commit^{tree}" 2>/dev/null; then
    git -C "$destination" fetch origin "$commit"
  fi
  git -C "$destination" cat-file -e "$commit^{tree}" 2>/dev/null \
    || die "cannot resolve $commit in $repository"
  git -C "$destination" checkout --detach "$commit"
}

apply_dependency_patch() {
  local source="$1"
  local patch="$2"

  need_file "$patch"
  git -C "$source" apply --check "$patch" \
    || die "dependency patch does not apply cleanly: $patch"
  git -C "$source" apply "$patch"
}

clean_env() {
  env -i PATH="$PATH" HOME="$HOME" LANG="${LANG:-en_US.UTF-8}" "$@"
}

codesign_file() {
  codesign --force --sign - --timestamp=none "$1"
}

write_pkgconfig() {
  local file="$1"
  local name="$2"
  local description="$3"
  local version="$4"
  local libraries="$5"

  mkdir -p "$(dirname "$file")"
  cat > "$file" <<EOF_PC
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: $name
Description: $description
Version: $version
Libs: -L\${libdir} $libraries
Cflags: -I\${includedir}
EOF_PC
}
