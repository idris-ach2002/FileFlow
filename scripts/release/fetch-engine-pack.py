#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST_ROOT = ROOT / "release/engines/packs"
MANIFEST = ROOT / "release/engines/manifest.json"


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def inventory_digest(items: list[dict[str, object]]) -> str:
    canonical = json.dumps(items, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def download(url: str, destination: Path) -> None:
    token = os.environ.get("FILEFLOW_ENGINE_PACK_TOKEN", "").strip() or os.environ.get("GITHUB_TOKEN", "").strip()
    headers = {"Accept": "application/octet-stream", "User-Agent": "FileFlow-engine-pack-fetcher/1"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request) as response, destination.open("wb") as output:
        shutil.copyfileobj(response, output)


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


def verify_pack(source: Path, target: str, pack_version: str) -> None:
    manifest_path = source / "pack-manifest.json"
    if not manifest_path.is_file():
        raise SystemExit("engine pack is missing pack-manifest.json")
    meta = json.loads(manifest_path.read_text())
    if meta.get("packVersion") != pack_version:
        raise SystemExit(f"engine pack version mismatch: {meta.get('packVersion')} != {pack_version}")
    if meta.get("target") != target:
        raise SystemExit(f"engine pack target mismatch: {meta.get('target')} != {target}")
    declared_items = meta.get("files", [])
    expected_content = str(meta.get("contentSha256", "")).strip().lower()
    if expected_content and inventory_digest(declared_items) != expected_content:
        raise SystemExit("engine pack manifest contentSha256 is invalid")

    declared: set[str] = set()
    for item in declared_items:
        rel = str(item.get("path", ""))
        path = safe_destination(source, rel)
        declared.add(rel)
        if not path.is_file():
            raise SystemExit(f"engine pack manifest references missing file: {rel}")
        actual = digest(path)
        if actual != str(item.get("sha256", "")).lower():
            raise SystemExit(f"engine pack file checksum mismatch: {rel}")
        if path.stat().st_size != int(item.get("size", -1)):
            raise SystemExit(f"engine pack file size mismatch: {rel}")

    actual_files = {
        path.relative_to(source).as_posix()
        for path in source.rglob("*")
        if path.is_file() and path.name != "pack-manifest.json"
    }
    if actual_files != declared:
        extra = sorted(actual_files - declared)
        missing = sorted(declared - actual_files)
        raise SystemExit(f"engine pack inventory mismatch; extra={extra}, missing={missing}")
    if not (source / "bin").is_dir():
        raise SystemExit("engine pack has no bin/ directory")


def install_archive(archive: Path, checksum_file: Path, target: str, pack_version: str) -> str:
    expected = checksum_file.read_text().strip().split()[0].lower()
    actual = digest(archive)
    if len(expected) != 64 or expected != actual:
        raise SystemExit(f"engine pack checksum mismatch: expected {expected}, got {actual}")

    with tempfile.TemporaryDirectory(prefix="fileflow-engine-pack-extract-") as temp:
        extracted = Path(temp) / "extracted"
        extracted.mkdir()
        extract(archive, extracted)
        children = [child for child in extracted.iterdir() if child.name != "__MACOSX"]
        source = extracted
        if not (source / "bin").is_dir() and len(children) == 1 and children[0].is_dir():
            source = children[0]
        verify_pack(source, target, pack_version)

        destination = DEST_ROOT / target
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(source, destination)
        print(f"engine pack {pack_version} verified -> {destination}")
        print(f"sha256 {actual}")
    return actual


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument(
        "--url-template",
        default=os.environ.get("FILEFLOW_ENGINE_PACK_URL_TEMPLATE", ""),
        help="Immutable URL containing {packVersion} and {target}",
    )
    parser.add_argument("--archive", type=Path, default=None, help="local immutable engine archive produced by make-engine-pack.py")
    parser.add_argument("--checksum", type=Path, default=None, help="checksum for --archive (defaults to <archive>.sha256)")
    args = parser.parse_args()

    release_manifest = json.loads(MANIFEST.read_text())
    pack_version = str(release_manifest.get("packVersion", "")).strip()

    if args.archive is not None:
        archive = args.archive.resolve()
        checksum = args.checksum.resolve() if args.checksum else Path(f"{archive}.sha256")
        if not archive.is_file() or not checksum.is_file():
            raise SystemExit(f"local engine pack/checksum missing: {archive} / {checksum}")
        install_archive(archive, checksum, args.target, pack_version)
        return

    template = args.url_template.strip()
    if "{target}" not in template or "{packVersion}" not in template:
        raise SystemExit("FILEFLOW_ENGINE_PACK_URL_TEMPLATE must contain {packVersion} and {target}")

    url = template.replace("{target}", args.target).replace("{packVersion}", pack_version)
    checksum_url = f"{url}.sha256"
    if url.endswith(".tar.gz"):
        suffix = ".tar.gz"
    elif url.endswith(".tgz"):
        suffix = ".tgz"
    elif url.endswith(".zip"):
        suffix = ".zip"
    else:
        raise SystemExit("engine pack URL must end with .tar.gz, .tgz or .zip")

    with tempfile.TemporaryDirectory(prefix="fileflow-engine-pack-download-") as temp:
        temp_root = Path(temp)
        archive = temp_root / f"pack{suffix}"
        checksum_file = temp_root / "pack.sha256"
        print(f"downloading immutable engine pack {url}")
        download(url, archive)
        download(checksum_url, checksum_file)
        install_archive(archive, checksum_file, args.target, pack_version)


if __name__ == "__main__":
    main()
