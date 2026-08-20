#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACK_ROOT = ROOT / "release/engines/packs"
OUT = ROOT / "release/engines/out"
MANIFEST = ROOT / "release/engines/manifest.json"


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def inventory(root: Path) -> list[dict[str, object]]:
    items: list[dict[str, object]] = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and path.name != "pack-manifest.json":
            items.append({
                "path": path.relative_to(root).as_posix(),
                "sha256": digest(path),
                "size": path.stat().st_size,
            })
    return items


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--source", type=Path, default=None)
    parser.add_argument("--licenses", type=Path, default=None, help="optional licenses directory to embed as licenses/")
    args = parser.parse_args()

    release_manifest = json.loads(MANIFEST.read_text())
    pack_version = str(release_manifest.get("packVersion", "")).strip()
    if not pack_version:
        raise SystemExit("release/engines/manifest.json is missing packVersion")

    source = args.source or (PACK_ROOT / args.target)
    if not (source / "bin").is_dir():
        raise SystemExit(f"missing engine pack directory: {source}/bin")

    OUT.mkdir(parents=True, exist_ok=True)
    base_name = f"fileflow-engines-{pack_version}-{args.target}"
    archive = OUT / f"{base_name}.tar.gz"

    with tempfile.TemporaryDirectory(prefix="fileflow-engine-pack-") as temp:
        staged = Path(temp) / base_name
        shutil.copytree(source, staged, symlinks=False)
        if args.licenses is not None:
            if not args.licenses.is_dir():
                raise SystemExit(f"missing licenses directory: {args.licenses}")
            destination = staged / "licenses"
            if destination.exists():
                shutil.rmtree(destination)
            shutil.copytree(args.licenses, destination, symlinks=False)
        pack_manifest = {
            "schemaVersion": 1,
            "packVersion": pack_version,
            "target": args.target,
            "files": inventory(staged),
        }
        (staged / "pack-manifest.json").write_text(json.dumps(pack_manifest, indent=2) + "\n")
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(staged, arcname=base_name)

    checksum = digest(archive)
    checksum_path = Path(f"{archive}.sha256")
    checksum_path.write_text(f"{checksum}  {archive.name}\n")
    print(archive)
    print(checksum_path)
    print(f"sha256 {checksum}")


if __name__ == "__main__":
    main()
