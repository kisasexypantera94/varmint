#!/usr/bin/env python3

import argparse
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "guest"))

from varmint_vet_catalog import catalog_json, compile_catalog  # noqa: E402


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    output = ROOT / "guest" / "varmint-vet-catalog.json"
    generated = catalog_json(compile_catalog(ROOT / "docs" / "games"))
    if arguments.check:
        if not output.is_file() or output.read_text(encoding="utf-8") != generated:
            print(
                f"{output.relative_to(ROOT)} is stale; run scripts/generate-vet-catalog.py",
                file=sys.stderr,
            )
            return 1
        return 0
    output.write_text(generated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
