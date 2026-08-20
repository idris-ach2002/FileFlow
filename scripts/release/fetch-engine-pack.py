#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST_ROOT = ROOT / "release/engines/packs"


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def safe_destination(root: Path, member: str) -> Path:
    candidate = (root / member).resolve()
    resolved_root = root.resolve()
    if candidate != resolved_root and resolved_root not in candidate.parents:
        raise SystemExit(f"unsafe engine pack path: {member}")
    return candidate


def extract(archive: Path, destination: Path) -> None:
    if archive.name.endswith((".tar.gz", ".tgz", ".tar.xz", ".tar.bz2")):
        with tarfile.open(archive, "r:*") as bundle:
            for member in bundle.getmembers():
                safe_destination(destination, member.name)
                if member.issym() or member.islnk():
                    raise SystemExit(f"engine pack links are not allowed: {member.name}")
            bundle.extractall(destination, filter="data")
        return
    if archive.suffix.lower() == ".zip":
        with zipfile.ZipFile(archive) as bundle:
            for info in bundle.infolist():
                safe_destination(destination, info.filename)
                unix_mode = (info.external_attr >> 16) & 0o170000
                if unix_mode == 0o120000:
                    raise SystemExit(f"engine pack links are not allowed: {info.filename}")
            bundle.extractall(destination)
        return
    raise SystemExit(f"unsupported engine pack archive: {archive.name}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument(
        "--url-template",
        default=os.environ.get("FILEFLOW_ENGINE_PACK_URL_TEMPLATE", ""),
        help="URL containing {target}, e.g. https://host/fileflow-engines-{target}.tar.gz",
    )
    args = parser.parse_args()
    template = args.url_template.strip()
    if not template or "{target}" not in template:
        raise SystemExit("FILEFLOW_ENGINE_PACK_URL_TEMPLATE must contain {target}")

    url = template.replace("{target}", args.target)
    checksum_url = f"{url}.sha256"
    if url.endswith('.tar.gz'):
        suffix = '.tar.gz'
    elif url.endswith('.tgz'):
        suffix = '.tgz'
    elif url.endswith('.zip'):
        suffix = '.zip'
    else:
        raise SystemExit('engine pack URL must end with .tar.gz, .tgz or .zip')

    with tempfile.TemporaryDirectory(prefix="fileflow-engine-pack-") as temp:
        temp_root = Path(temp)
        archive = temp_root / f"pack{suffix}"
        checksum_file = temp_root / "pack.sha256"
        print(f"downloading {url}")
        urllib.request.urlretrieve(url, archive)
        urllib.request.urlretrieve(checksum_url, checksum_file)
        expected = checksum_file.read_text().strip().split()[0].lower()
        actual = digest(archive)
        if len(expected) != 64 or expected != actual:
            raise SystemExit(f"engine pack checksum mismatch: expected {expected}, got {actual}")

        extracted = temp_root / "extracted"
        extracted.mkdir()
        extract(archive, extracted)

        # Accept an archive rooted directly at bin/lib/share, or one wrapper directory.
        children = [child for child in extracted.iterdir() if child.name != "__MACOSX"]
        source = extracted
        if not (source / "bin").is_dir() and len(children) == 1 and children[0].is_dir():
            source = children[0]
        if not (source / "bin").is_dir():
            raise SystemExit("engine pack has no bin/ directory")

        destination = DEST_ROOT / args.target
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(source, destination)
        print(f"engine pack verified -> {destination}")
        print(f"sha256 {actual}")


if __name__ == "__main__":
    main()
