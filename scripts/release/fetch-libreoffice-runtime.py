#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "release/engines/libreoffice-runtime.json"
VENDOR_ROOT = ROOT / "release/engines/vendor/libreoffice"
USER_AGENT = "FileFlow-engine-factory/1.0"


def log(message: str) -> None:
    print(f"[libreoffice-source] {message}", flush=True)


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def download(url: str, destination: Path, attempts: int = 4) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_suffix(destination.suffix + ".part")
    last: Exception | None = None
    for attempt in range(1, attempts + 1):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(request, timeout=90) as response, partial.open("wb") as handle:
                shutil.copyfileobj(response, handle, length=1024 * 1024)
            partial.replace(destination)
            return
        except (OSError, urllib.error.URLError) as error:
            last = error
            partial.unlink(missing_ok=True)
            if attempt < attempts:
                time.sleep(attempt * 2)
    raise SystemExit(f"download failed after {attempts} attempts: {url}: {last}")


def fetch_text(url: str) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read().decode("utf-8", "replace")
    except (OSError, urllib.error.URLError) as error:
        raise SystemExit(f"unable to fetch checksum metadata {url}: {error}") from error


def official_sha256(url: str, configured: str | None) -> str:
    # Even when a checksum is pinned in git, compare it with TDF's sidecar. This
    # catches a stale/mistyped recipe before downloading hundreds of megabytes.
    sidecar = fetch_text(url + ".sha256")
    match = re.search(r"\b([0-9a-fA-F]{64})\b", sidecar)
    if not match:
        raise SystemExit(f"official SHA-256 sidecar is malformed: {url}.sha256")
    remote = match.group(1).lower()
    if configured:
        pinned = configured.strip().lower()
        if pinned != remote:
            raise SystemExit(
                "LibreOffice checksum recipe disagrees with The Document Foundation: "
                f"pinned={pinned} official={remote}"
            )
        return pinned
    return remote


def safe_extract_tar(archive: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    root = destination.resolve()
    with tarfile.open(archive, "r:*") as tar:
        for member in tar.getmembers():
            target = (destination / member.name).resolve(strict=False)
            try:
                target.relative_to(root)
            except ValueError as error:
                raise SystemExit(f"unsafe path in LibreOffice archive: {member.name}") from error
        tar.extractall(destination)


def copytree_preserve(src: Path, dst: Path) -> None:
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst, symlinks=True)


def extract_linux(archive: Path, destination: Path, workspace: Path) -> Path:
    expanded = workspace / "archive"
    sysroot = workspace / "sysroot"
    safe_extract_tar(archive, expanded)
    debs = sorted(expanded.rglob("*.deb"))
    if not debs:
        raise SystemExit("official LibreOffice Linux archive contains no .deb packages")
    sysroot.mkdir(parents=True)
    for deb in debs:
        subprocess.run(["dpkg-deb", "-x", str(deb), str(sysroot)], check=True)
    candidates = sorted(
        path for path in (sysroot / "opt").glob("libreoffice*")
        if (path / "program" / "soffice").is_file()
    )
    if len(candidates) != 1:
        raise SystemExit(f"expected one official LibreOffice /opt tree, found: {candidates}")
    copytree_preserve(candidates[0], destination)
    return destination / "program" / "soffice"


def extract_macos(archive: Path, destination: Path, workspace: Path) -> Path:
    if shutil.which("hdiutil") is None:
        raise SystemExit("hdiutil is required to extract the official LibreOffice DMG")
    mount = workspace / "mount"
    mount.mkdir()
    subprocess.run(
        ["hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", str(mount), str(archive)],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    try:
        apps = sorted(mount.glob("LibreOffice.app"))
        if len(apps) != 1:
            raise SystemExit(f"LibreOffice.app missing from official DMG: {apps}")
        copytree_preserve(apps[0] / "Contents", destination / "Contents")
    finally:
        subprocess.run(["hdiutil", "detach", str(mount), "-force"], check=False)
    return destination / "Contents" / "MacOS" / "soffice"


def extract_windows(archive: Path, destination: Path, workspace: Path) -> Path:
    msiexec = shutil.which("msiexec.exe") or shutil.which("msiexec")
    if not msiexec:
        raise SystemExit("msiexec is required to extract the official LibreOffice MSI")
    administrative = workspace / "msi-root"
    administrative.mkdir()
    subprocess.run(
        [msiexec, "/a", str(archive), "/qn", f"TARGETDIR={administrative}"],
        check=True,
    )
    launchers = sorted(administrative.rglob("soffice.exe"), key=lambda path: (len(path.parts), str(path).lower()))
    if not launchers:
        raise SystemExit("soffice.exe missing after administrative MSI extraction")
    source = launchers[0].parent.parent
    if not (source / "program" / "soffice.exe").is_file():
        raise SystemExit(f"unable to identify LibreOffice root from {launchers[0]}")
    copytree_preserve(source, destination)
    return destination / "program" / "soffice.exe"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--output-root", type=Path, default=VENDOR_ROOT)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    entry = manifest.get("targets", {}).get(args.target)
    if not isinstance(entry, dict):
        raise SystemExit(f"no official LibreOffice runtime recipe for {args.target}")
    version = str(manifest.get("version", "")).strip()
    url = str(entry.get("url", "")).strip()
    kind = str(entry.get("kind", "")).strip()
    configured_sha = entry.get("sha256")
    if not version or not url or not kind:
        raise SystemExit(f"incomplete LibreOffice runtime recipe for {args.target}")
    if not url.startswith("https://download.documentfoundation.org/"):
        raise SystemExit(f"refusing non-TDF LibreOffice source URL: {url}")

    expected_sha = official_sha256(url, str(configured_sha) if configured_sha else None)
    destination = args.output_root / args.target
    if destination.exists():
        metadata = destination / ".fileflow-source.json"
        if metadata.is_file():
            existing = json.loads(metadata.read_text(encoding="utf-8"))
            if existing.get("sha256") == expected_sha and existing.get("url") == url:
                log(f"already prepared {args.target} LibreOffice {version} sha256={expected_sha}")
                return
        shutil.rmtree(destination)

    with tempfile.TemporaryDirectory(prefix="fileflow-libreoffice-") as temp:
        workspace = Path(temp)
        filename = url.rsplit("/", 1)[-1]
        archive = workspace / filename
        log(f"downloading official LibreOffice {version} for {args.target}")
        download(url, archive)
        actual_sha = digest(archive)
        if actual_sha != expected_sha:
            raise SystemExit(
                f"LibreOffice SHA-256 mismatch for {args.target}: expected={expected_sha} actual={actual_sha}"
            )
        log(f"verified sha256={actual_sha}")
        destination.parent.mkdir(parents=True, exist_ok=True)
        if kind == "linux-deb-tar":
            launcher = extract_linux(archive, destination, workspace)
        elif kind == "macos-dmg":
            launcher = extract_macos(archive, destination, workspace)
        elif kind == "windows-msi":
            launcher = extract_windows(archive, destination, workspace)
        else:
            raise SystemExit(f"unsupported LibreOffice source kind: {kind}")

    metadata = {
        "schemaVersion": 1,
        "version": version,
        "target": args.target,
        "kind": kind,
        "url": url,
        "sha256": expected_sha,
        "launcher": launcher.relative_to(destination).as_posix(),
    }
    (destination / ".fileflow-source.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    log(f"prepared {args.target} -> {destination.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
