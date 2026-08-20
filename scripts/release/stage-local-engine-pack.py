#!/usr/bin/env python3
from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def run(script: str, *args: str) -> None:
    subprocess.run([sys.executable, str(ROOT / script), *args], cwd=ROOT, check=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--directory", type=Path, default=ROOT / "release/engines/out")
    parser.add_argument("--mode", choices=["optional", "core", "full"], default="full")
    args = parser.parse_args()

    directory = args.directory.resolve()
    matches = sorted(directory.glob(f"fileflow-engines-*-{args.target}.tar.gz"))
    if len(matches) != 1:
        raise SystemExit(f"expected exactly one engine archive for {args.target} in {directory}, found {len(matches)}")
    archive = matches[0]
    checksum = Path(f"{archive}.sha256")
    if not checksum.is_file():
        raise SystemExit(f"missing engine archive checksum: {checksum}")

    run("scripts/release/fetch-engine-pack.py", "--target", args.target, "--archive", str(archive), "--checksum", str(checksum))
    run("scripts/release/stage-engines.py", "--target", args.target, "--source", str(ROOT / "release/engines/packs"), "--mode", args.mode)


if __name__ == "__main__":
    main()
