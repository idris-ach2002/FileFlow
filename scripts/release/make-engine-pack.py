#!/usr/bin/env python3
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import shutil
import tarfile
import tempfile
from collections import defaultdict
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


def inventory_digest(items: list[dict[str, object]]) -> str:
    canonical = json.dumps(items, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def report_size(root: Path, items: list[dict[str, object]]) -> int:
    total = sum(int(item["size"]) for item in items)
    print(f"engine pack inventory: {len(items)} files, {total / (1024 ** 2):.1f} MiB")
    biggest = sorted(items, key=lambda item: int(item["size"]), reverse=True)[:10]
    print("largest files:")
    for item in biggest:
        print(f"  {int(item['size']) / (1024 ** 2):8.1f} MiB  {item['path']}")

    directories: dict[str, int] = defaultdict(int)
    for item in items:
        parts = Path(str(item["path"])).parts
        for depth in range(1, min(len(parts), 4)):
            directories["/".join(parts[:depth])] += int(item["size"])
    print("largest directories:")
    for name, size in sorted(directories.items(), key=lambda pair: pair[1], reverse=True)[:10]:
        print(f"  {size / (1024 ** 2):8.1f} MiB  {name}/")
    return total


def normalized_tarinfo(info: tarfile.TarInfo) -> tarfile.TarInfo:
    info.uid = 0
    info.gid = 0
    info.uname = "root"
    info.gname = "root"
    info.mtime = 0
    if info.isdir():
        info.mode = 0o755
    elif info.isfile():
        info.mode = 0o755 if info.mode & 0o111 else 0o644
    return info


def write_reproducible_tar_gz(source: Path, archive: Path, arcname: str) -> None:
    with archive.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w", format=tarfile.PAX_FORMAT) as bundle:
                bundle.add(source, arcname=arcname, recursive=True, filter=normalized_tarinfo)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--source", type=Path, default=None)
    parser.add_argument("--licenses", type=Path, default=None, help="optional licenses directory to embed as licenses/")
    parser.add_argument(
        "--max-bytes",
        type=int,
        default=int(os.environ.get("FILEFLOW_ENGINE_PACK_MAX_BYTES", "0") or "0"),
        help="optional uncompressed pack size ceiling (0 disables)",
    )
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

        items = inventory(staged)
        total = report_size(staged, items)
        if args.max_bytes and total > args.max_bytes:
            raise SystemExit(f"engine pack exceeds configured limit: {total} > {args.max_bytes} bytes")
        pack_manifest = {
            "schemaVersion": 2,
            "packVersion": pack_version,
            "target": args.target,
            "contentSha256": inventory_digest(items),
            "files": items,
        }
        (staged / "pack-manifest.json").write_text(json.dumps(pack_manifest, indent=2, sort_keys=True) + "\n")
        write_reproducible_tar_gz(staged, archive, base_name)

    checksum = digest(archive)
    checksum_path = Path(f"{archive}.sha256")
    checksum_path.write_text(f"{checksum}  {archive.name}\n")
    print(archive)
    print(checksum_path)
    print(f"sha256 {checksum}")


if __name__ == "__main__":
    main()
