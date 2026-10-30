#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="$ROOT/build/prefix"

usage() {
    echo "usage: $0 run <output.json>" >&2
    echo "       $0 compare <baseline.json> <candidate.json>" >&2
    exit 2
}

run() {
    local output frameworks
    output="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"

    # ANGLE's libEGL loads libGLESv2 by framework path, as laid out in the app
    # bundle. Point that at the prefix build so both resolve to one image.
    frameworks="$ROOT/build/present-bench/Frameworks"
    mkdir -p "$frameworks/GLESv2.framework/Versions/A"
    ln -sf "$PREFIX/lib/libGLESv2.dylib" "$frameworks/GLESv2.framework/Versions/A/GLESv2"
    ln -sfn A "$frameworks/GLESv2.framework/Versions/Current"

    CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/build/cargo}" \
        PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}" \
        RUSTFLAGS="-L native=$PREFIX/lib ${RUSTFLAGS:-}" \
        DYLD_LIBRARY_PATH="$PREFIX/lib" \
        DYLD_FRAMEWORK_PATH="$frameworks" \
        VARMINT_PRESENT_BENCH_OUTPUT="$output" \
        cargo test --release --manifest-path "$ROOT/Cargo.toml" display::angle::tests -- --ignored --test-threads=1

    python3 - "$output" "$(git -C "$ROOT" rev-parse --short HEAD)$(git -C "$ROOT" diff --quiet HEAD -- src || echo -dirty)" <<'PY'
import json, sys
path, revision = sys.argv[1:]
report = json.load(open(path))
report = {"revision": revision, **report}
json.dump(report, open(path, "w"), indent=2)
for case in report["cases"]:
    gpu = case.get("gpu_execution_median_ms")
    print(f'{case["implementation"]:<11} {case["name"]:<36} cpu median {case["copy_cpu_median_ms"]:.3f} ms  '
          f'p95 {case["copy_cpu_p95_ms"]:.3f} ms  completion {case["completion_latency_median_ms"]:.3f} ms  '
          f'gpu {"-" if gpu is None else "%.3f ms" % gpu}  '
          f'source rebuilds/copy {case["source_rebuilds_per_copy"]:.2f}')
PY
}

compare() {
    python3 - "$1" "$2" <<'PY'
import json, sys
base, cand = (json.load(open(p)) for p in sys.argv[1:])
print(f'baseline {base.get("revision")}  candidate {cand.get("revision")}')
print(f'{"case":<40} {"cpu median ms":>22} {"delta":>8} {"run medians (base | cand)":>34} {"src rebuilds/copy":>18}')
key = lambda case: (case.get("implementation", "angle-copy"), case["name"])
cand_cases = {key(c): c for c in cand["cases"]}
for b in base["cases"]:
    c = cand_cases.get(key(b))
    if c is None:
        continue
    spread = lambda case: (min(r["copy_cpu_ms"]["median"] for r in case["runs"]),
                           max(r["copy_cpu_ms"]["median"] for r in case["runs"]))
    bm, cm = b["copy_cpu_median_ms"], c["copy_cpu_median_ms"]
    print(f'{" ".join(key(b)):<40} {bm:>10.3f} -> {cm:<9.3f} {100 * (cm - bm) / bm:>+7.1f}% '
          f'{"%.3f-%.3f | %.3f-%.3f" % (*spread(b), *spread(c)):>34} '
          f'{b["source_rebuilds_per_copy"]:>8.2f} -> {c["source_rebuilds_per_copy"]:.2f}')
PY
}

case "${1:-}" in
    run) [ "$#" -eq 2 ] || usage; run "$2" ;;
    compare) [ "$#" -eq 3 ] || usage; compare "$2" "$3" ;;
    *) usage ;;
esac
