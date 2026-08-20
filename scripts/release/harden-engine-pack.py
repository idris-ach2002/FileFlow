#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ENGINE_ROOT = ROOT / "src-tauri/resources/engines"
BIN = ENGINE_ROOT / "bin"
LIB = ENGINE_ROOT / "lib"
META = ROOT / "src-tauri/resources/engine-pack.json"


def run(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=check)


def host_family() -> str:
    if sys.platform == "darwin": return "macos"
    if sys.platform == "win32": return "windows"
    if sys.platform.startswith("linux"): return "linux"
    return sys.platform


def target_family(target: str) -> str:
    if "apple-darwin" in target: return "macos"
    if "windows" in target: return "windows"
    if "linux" in target: return "linux"
    raise SystemExit(f"unsupported target: {target}")


def all_files() -> list[Path]:
    return [p for base in (BIN, LIB) if base.is_dir() for p in base.rglob("*") if p.is_file()]


def file_kind(path: Path) -> str:
    if shutil.which("file") is None:
        return ""
    return run("file", "-b", str(path), check=False).stdout


def macos_harden(identity: str) -> None:
    install_name_tool = shutil.which("install_name_tool")
    codesign = shutil.which("codesign")
    otool = shutil.which("otool")
    if not install_name_tool or not otool:
        raise SystemExit("macOS hardening requires install_name_tool and otool")
    internal = {p.name: p for p in all_files() if "Mach-O" in file_kind(p)}
    for path in internal.values():
        kind = file_kind(path)
        if path.suffix == ".dylib":
            run(install_name_tool, "-id", f"@rpath/{path.name}", str(path))
        deps = run(otool, "-L", str(path)).stdout.splitlines()[1:]
        for line in deps:
            dep = line.strip().split(" (", 1)[0]
            if not dep.startswith("/"):
                continue
            name = Path(dep).name
            if name not in internal:
                continue
            replacement = f"@loader_path/{name}" if path.parent == LIB else f"@loader_path/../lib/{name}"
            run(install_name_tool, "-change", dep, replacement, str(path))
    if identity:
        if not codesign:
            raise SystemExit("codesign is required for macOS engine signing")
        # Libraries first, executables last.
        for path in sorted(internal.values(), key=lambda p: (p.parent != LIB, str(p))):
            if identity == "-":
                run(codesign, "--force", "--sign", identity, str(path))
            else:
                run(codesign, "--force", "--options", "runtime", "--timestamp", "--sign", identity, str(path))


def linux_harden() -> None:
    patchelf = shutil.which("patchelf")
    if not patchelf:
        raise SystemExit("Linux hardening requires patchelf")
    readelf = shutil.which("readelf")
    if not readelf:
        raise SystemExit("Linux hardening requires readelf")
    for path in all_files():
        if "ELF" not in file_kind(path):
            continue
        dynamic = run(readelf, "-d", str(path), check=False)
        # Fully static ELF tools have no dynamic section and need no RPATH.
        if dynamic.returncode != 0 or "There is no dynamic section" in dynamic.stdout:
            continue
        rpath = "$ORIGIN:$ORIGIN/../lib" if path.parent == BIN else "$ORIGIN"
        result = run(patchelf, "--set-rpath", rpath, str(path), check=False)
        if result.returncode != 0:
            raise SystemExit(f"patchelf failed for {path}: {result.stdout}")


def windows_harden() -> None:
    # Keep DLLs in lib/ and make the runtime add both bin/ and lib/ to PATH.
    # No binary rewriting is required; PE architecture/signature validation is
    # performed separately after certificate import.
    return


def sha256(path: Path) -> str:
    h=hashlib.sha256(); h.update(path.read_bytes()); return h.hexdigest()


def refresh_metadata() -> None:
    if not META.is_file(): return
    meta=json.loads(META.read_text())
    for item in meta.get("engines", []):
        path=BIN / item["name"]
        if path.is_file(): item["sha256"] = sha256(path)
    meta["hardened"] = True
    META.write_text(json.dumps(meta, indent=2)+"\n")


def main() -> None:
    parser=argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--sign-identity", default=os.environ.get("APPLE_SIGNING_IDENTITY", ""))
    args=parser.parse_args()
    family=target_family(args.target)
    if family != host_family():
        raise SystemExit(f"engine hardening for {args.target} must run natively on {family}")
    if family == "macos": macos_harden(args.sign_identity.strip())
    elif family == "linux": linux_harden()
    else: windows_harden()
    refresh_metadata()
    print(f"native engine pack hardened for {args.target}")


if __name__ == "__main__":
    main()
