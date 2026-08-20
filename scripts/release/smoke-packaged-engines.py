#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def walk(root: Path):
    if not root.exists():
        return []
    return [path for path in root.rglob("*")]


def find_resource_pair(root: Path) -> tuple[Path, Path]:
    metadata = sorted(root.rglob("engine-pack.json"), key=lambda p: (len(p.parts), str(p)))
    for meta in metadata:
        engine_root = meta.parent / "engines"
        if (engine_root / "bin").is_dir():
            return engine_root, meta
    engines = sorted((p for p in root.rglob("engines") if (p / "bin").is_dir()), key=lambda p: (len(p.parts), str(p)))
    for engine_root in engines:
        candidates = [engine_root.parent / "engine-pack.json", engine_root / "engine-pack.json"]
        meta = next((p for p in candidates if p.is_file()), None)
        if meta:
            return engine_root, meta
    raise SystemExit(f"packaged engine resources not found below {root}")


def bundle_root(target: str) -> Path:
    root = ROOT / "target" / target / "release" / "bundle"
    if not root.is_dir():
        raise SystemExit(f"bundle root missing: {root}")
    return root


def find_app(bundle: Path) -> Path:
    apps = sorted((p for p in bundle.rglob("*.app") if p.is_dir()), key=lambda p: (len(p.parts), str(p)))
    if not apps:
        raise SystemExit("packaged macOS .app not found")
    return apps[0]


def find_appimage(bundle: Path) -> Path:
    matches = sorted((p for p in bundle.rglob("*") if p.is_file() and p.name.lower().endswith(".appimage")))
    if not matches:
        raise SystemExit("packaged AppImage not found")
    return matches[0]


def find_nsis(bundle: Path) -> Path:
    executables = sorted((p for p in bundle.rglob("*.exe") if p.is_file()))
    preferred = [p for p in executables if "nsis" in p.name.lower() or "setup" in p.name.lower()]
    if preferred:
        return preferred[0]
    if executables:
        return executables[0]
    raise SystemExit("packaged NSIS installer not found")


def run_functional(engine_root: Path, metadata: Path, mode: str) -> None:
    print(f"[packaged-engines] resource root: {engine_root}")
    subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/release/functional-engine-tests.py"),
            "--mode", mode,
            "--engine-root", str(engine_root),
            "--metadata", str(metadata),
        ],
        cwd=ROOT,
        check=True,
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--mode", choices=["optional", "core", "full"], default="full")
    args = parser.parse_args()
    bundle = bundle_root(args.target)

    if sys.platform == "darwin":
        app = find_app(bundle)
        engine_root, metadata = find_resource_pair(app / "Contents" / "Resources")
        run_functional(engine_root, metadata, args.mode)
        return

    with tempfile.TemporaryDirectory(prefix="fileflow-packaged-engines-") as temp:
        temp_root = Path(temp)
        if sys.platform.startswith("linux"):
            appimage = find_appimage(bundle)
            env = dict(os.environ)
            env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
            result = subprocess.run(
                [str(appimage), "--appimage-extract"],
                cwd=temp_root,
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=180,
            )
            if result.returncode != 0:
                raise SystemExit(f"AppImage extraction failed ({result.returncode}): {(result.stdout or '')[-2000:]}")
            extracted = temp_root / "squashfs-root"
            engine_root, metadata = find_resource_pair(extracted)
            run_functional(engine_root, metadata, args.mode)
            return

        if os.name == "nt":
            installer = find_nsis(bundle)
            install_root = temp_root / "installed"
            result = subprocess.run(
                [str(installer), "/S", f"/D={install_root}"],
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=180,
            )
            if result.returncode != 0:
                raise SystemExit(f"NSIS install failed ({result.returncode}): {(result.stdout or '')[-2000:]}")
            engine_root, metadata = find_resource_pair(install_root)
            run_functional(engine_root, metadata, args.mode)
            return

    raise SystemExit(f"unsupported packaged-engine smoke host: {sys.platform}")


if __name__ == "__main__":
    main()
