#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACK_ROOT = ROOT / "release/engines/packs"
OUT = ROOT / "release/engines/out"


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    source = PACK_ROOT / args.target
    if not (source / "bin").is_dir():
        raise SystemExit(f"missing engine pack directory: {source}/bin")
    OUT.mkdir(parents=True, exist_ok=True)
    archive = OUT / f"fileflow-engines-{args.target}.tar.gz"
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(source, arcname=f"fileflow-engines-{args.target}")
    checksum = digest(archive)
    checksum_path = Path(f"{archive}.sha256")
    checksum_path.write_text(f"{checksum}  {archive.name}\n")
    print(archive)
    print(checksum_path)
    print(f"sha256 {checksum}")


if __name__ == "__main__":
    main()
