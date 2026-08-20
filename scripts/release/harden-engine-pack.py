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
META = ROOT / "src-tauri/resources/engine-pack.json"


def run(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=check,
    )


def host_family() -> str:
    if sys.platform == "darwin":
        return "macos"
    if sys.platform == "win32":
        return "windows"
    if sys.platform.startswith("linux"):
        return "linux"
    return sys.platform


def target_family(target: str) -> str:
    if "apple-darwin" in target:
        return "macos"
    if "windows" in target:
        return "windows"
    if "linux" in target:
        return "linux"
    raise SystemExit(f"unsupported target: {target}")


def file_kind(path: Path) -> str:
    tool = shutil.which("file")
    if tool is None:
        return ""
    return run(tool, "-b", str(path), check=False).stdout


def native_candidates() -> list[Path]:
    result = []
    for path in ENGINE_ROOT.rglob("*"):
        if not path.is_file():
            continue
        lower = path.name.lower()
        executable = bool(path.stat().st_mode & 0o111)
        native_suffix = (
            lower.endswith((".exe", ".dll", ".dylib", ".so"))
            or ".so." in lower
        )
        if executable or native_suffix:
            result.append(path)
    return result


def internal_native_files(kind: str) -> list[Path]:
    token = "Mach-O" if kind == "macos" else "ELF"
    return [
        path
        for path in native_candidates()
        if token in file_kind(path)
    ]


def loader_relative(source: Path, target: Path) -> str:
    relative = os.path.relpath(target, source.parent).replace(os.sep, "/")
    return (
        "@loader_path"
        if relative == "."
        else f"@loader_path/{relative}"
    )


def macos_harden(identity: str) -> None:
    install_name_tool = shutil.which("install_name_tool")
    codesign = shutil.which("codesign")
    otool = shutil.which("otool")
    if not install_name_tool or not otool:
        raise SystemExit(
            "macOS hardening requires install_name_tool and otool"
        )

    native = internal_native_files("macos")
    by_name: dict[str, list[Path]] = {}
    for path in native:
        by_name.setdefault(path.name, []).append(path)

    for path in native:
        if path.suffix == ".dylib":
            run(
                install_name_tool,
                "-id",
                f"@rpath/{path.name}",
                str(path),
            )

        deps = run(otool, "-L", str(path)).stdout.splitlines()[1:]
        for line in deps:
            dep = line.strip().split(" (", 1)[0]
            name = Path(dep).name
            candidates = by_name.get(name, [])
            if not candidates:
                continue
            target = sorted(
                candidates,
                key=lambda candidate: (
                    0 if candidate.parent == path.parent else 1,
                    len(candidate.parts),
                    str(candidate),
                ),
            )[0]
            replacement = loader_relative(path, target)
            if dep != replacement and (
                dep.startswith("/")
                or dep.startswith("@rpath/")
                or dep.startswith("@loader_path/")
                or dep.startswith("@executable_path/")
            ):
                run(
                    install_name_tool,
                    "-change",
                    dep,
                    replacement,
                    str(path),
                )

    if identity:
        if not codesign:
            raise SystemExit(
                "codesign is required for macOS engine signing"
            )
        for path in sorted(
            native,
            key=lambda p: (-len(p.parts), str(p)),
        ):
            if identity == "-":
                run(codesign, "--force", "--sign", identity, str(path))
            else:
                run(
                    codesign,
                    "--force",
                    "--options",
                    "runtime",
                    "--timestamp",
                    "--sign",
                    identity,
                    str(path),
                )


def origin_to(source: Path, destination: Path) -> str:
    relative = os.path.relpath(
        destination,
        source.parent,
    ).replace(os.sep, "/")
    return "$ORIGIN" if relative == "." else f"$ORIGIN/{relative}"


def linux_harden() -> None:
    patchelf = shutil.which("patchelf")
    readelf = shutil.which("readelf")
    if not patchelf or not readelf:
        raise SystemExit(
            "Linux hardening requires patchelf and readelf"
        )

    runtime_lib = ENGINE_ROOT / "share" / "runtime" / "lib"
    runtime_library_bin = (
        ENGINE_ROOT / "share" / "runtime" / "Library" / "bin"
    )
    office_program = (
        ENGINE_ROOT / "share" / "libreoffice" / "program"
    )

    for path in internal_native_files("linux"):
        dynamic = run(readelf, "-d", str(path), check=False)
        if (
            dynamic.returncode != 0
            or "There is no dynamic section" in dynamic.stdout
        ):
            continue

        dirs = [path.parent]
        for directory in (
            runtime_lib,
            runtime_library_bin,
            office_program,
        ):
            if directory.is_dir() and directory not in dirs:
                dirs.append(directory)

        rpath = ":".join(
            dict.fromkeys(
                origin_to(path, directory)
                for directory in dirs
            )
        )
        result = run(
            patchelf,
            "--set-rpath",
            rpath,
            str(path),
            check=False,
        )
        if result.returncode != 0:
            raise SystemExit(
                f"patchelf failed for {path}: {result.stdout}"
            )


def windows_harden() -> None:
    # Private launcher PATH plus validate-engine-pack's PE closure gate.
    return


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(
            lambda: handle.read(1024 * 1024),
            b"",
        ):
            h.update(block)
    return h.hexdigest()


def refresh_metadata() -> None:
    if not META.is_file():
        return
    meta = json.loads(META.read_text())
    for item in meta.get("engines", []):
        path = BIN / item["name"]
        if path.is_file():
            item["sha256"] = sha256(path)
    meta["hardened"] = True
    META.write_text(
        json.dumps(meta, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument(
        "--sign-identity",
        default=os.environ.get("APPLE_SIGNING_IDENTITY", ""),
    )
    args = parser.parse_args()

    family = target_family(args.target)
    if family != host_family():
        raise SystemExit(
            f"engine hardening for {args.target} "
            f"must run natively on {family}"
        )

    if family == "macos":
        macos_harden(args.sign_identity.strip())
    elif family == "linux":
        linux_harden()
    else:
        windows_harden()

    refresh_metadata()
    print(
        f"native FULL engine runtime hardened for {args.target}"
    )


if __name__ == "__main__":
    main()
