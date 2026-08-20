#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "release/engines/manifest.json"
ENGINE_ROOT = ROOT / "src-tauri/resources/engines"
BIN_DEST = ENGINE_ROOT / "bin"
LIB_DEST = ENGINE_ROOT / "lib"
SHARE_DEST = ENGINE_ROOT / "share"
META = ROOT / "src-tauri/resources/engine-pack.json"
LICENSE_DEST = ROOT / "src-tauri/resources/licenses"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def executable_variants(name: str, target: str) -> list[str]:
    if "windows" in target and not name.lower().endswith(".exe"):
        return [f"{name}.exe", name]
    return [name]


def copy_tree(source: Path, destination: Path) -> None:
    if not source.is_dir():
        return
    shutil.copytree(source, destination)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument(
        "--source",
        type=Path,
        default=Path(os.environ.get("FILEFLOW_ENGINE_PACK_DIR", "release/engines/packs")),
    )
    parser.add_argument(
        "--mode",
        choices=["optional", "core", "full"],
        default=os.environ.get("FILEFLOW_ENGINE_MODE", "optional") or "optional",
    )
    args = parser.parse_args()

    source = args.source / args.target if (args.source / args.target).is_dir() else args.source
    manifest = json.loads(MANIFEST.read_text())
    pack_version = str(manifest.get("packVersion", "")).strip()

    for directory in (BIN_DEST, LIB_DEST, SHARE_DEST, LICENSE_DEST):
        if directory.exists():
            shutil.rmtree(directory)
    BIN_DEST.mkdir(parents=True)
    LICENSE_DEST.mkdir(parents=True)

    pack_meta_path = source / "pack-manifest.json"
    source_pack_meta = None
    if pack_meta_path.is_file():
        source_pack_meta = json.loads(pack_meta_path.read_text())
        if source_pack_meta.get("packVersion") != pack_version:
            raise SystemExit("staged engine pack version does not match release manifest")
        if source_pack_meta.get("target") != args.target:
            raise SystemExit("staged engine pack target does not match requested target")
    elif args.mode != "optional":
        raise SystemExit("core/full release requires an immutable pack-manifest.json")

    source_bin = source / "bin"
    if source_bin.is_dir():
        for item in source_bin.iterdir():
            target = BIN_DEST / item.name
            if item.is_dir():
                shutil.copytree(item, target)
            else:
                shutil.copy2(item, target)
                if os.name != "nt":
                    target.chmod(target.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    copy_tree(source / "lib", LIB_DEST)
    copy_tree(source / "share", SHARE_DEST)

    staged: list[dict[str, str]] = []
    missing: list[str] = []
    for engine in manifest["engines"]:
        for executable in engine["executables"]:
            found = next((BIN_DEST / candidate for candidate in executable_variants(executable, args.target) if (BIN_DEST / candidate).is_file()), None)
            required = args.mode == "full" or (args.mode == "core" and engine["tier"] == "core")
            if found is None:
                if required:
                    missing.append(f"{engine['id']}:{executable}")
                continue
            staged.append({
                "engine": engine["id"],
                "name": found.name,
                "sha256": sha256(found),
                "license": engine["license"],
                "tier": engine["tier"],
            })

        license_file = source / "licenses" / f"{engine['id']}.txt"
        if license_file.is_file():
            shutil.copy2(license_file, LICENSE_DEST / license_file.name)

    META.write_text(json.dumps({
        "schemaVersion": 2,
        "packVersion": pack_version,
        "target": args.target,
        "flavor": args.mode,
        "sourceManifest": source_pack_meta,
        "engines": staged,
    }, indent=2) + "\n")
    print(f"staged {len(staged)} engine executable(s) from pack {pack_version} for {args.target} ({args.mode})")
    if missing:
        print("missing required engines: " + ", ".join(missing))
        raise SystemExit(2)


if __name__ == "__main__":
    main()
