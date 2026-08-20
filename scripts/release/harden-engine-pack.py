#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ENGINE_ROOT = ROOT / "src-tauri/resources/engines"
BIN = ENGINE_ROOT / "bin"
META = ROOT / "src-tauri/resources/engine-pack.json"
WINDOWS_PATHS = ENGINE_ROOT / "engine-runtime-paths.txt"

SYSTEM_MAC_PREFIXES = ("/System/Library/", "/usr/lib/")


def run(*args: str, check: bool = True, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=check,
        env=env,
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
    result: list[Path] = []
    for path in ENGINE_ROOT.rglob("*"):
        if not path.is_file():
            continue
        lower = path.name.lower()
        executable = bool(path.stat().st_mode & 0o111)
        native_suffix = lower.endswith((".exe", ".dll", ".dylib", ".so")) or ".so." in lower
        if executable or native_suffix:
            result.append(path)
    return result


def internal_native_files(kind: str) -> list[Path]:
    if kind == "windows":
        return [p for p in native_candidates() if p.suffix.lower() in {".exe", ".dll"}]
    token = "Mach-O" if kind == "macos" else "ELF"
    return [path for path in native_candidates() if token in file_kind(path)]


def inside_engine_root(path: Path) -> bool:
    try:
        path.resolve(strict=False).relative_to(ENGINE_ROOT.resolve())
        return True
    except ValueError:
        return False


def loader_relative(source: Path, target: Path) -> str:
    relative = os.path.relpath(target, source.parent).replace(os.sep, "/")
    return "@loader_path" if relative == "." else f"@loader_path/{relative}"


def origin_to(source: Path, destination: Path) -> str:
    relative = os.path.relpath(destination, source.parent).replace(os.sep, "/")
    return "$ORIGIN" if relative == "." else f"$ORIGIN/{relative}"


def expand_origin(source: Path, entry: str) -> Path | None:
    value = entry.replace("${ORIGIN}", "$ORIGIN")
    if value == "$ORIGIN":
        candidate = source.parent
    elif value.startswith("$ORIGIN/"):
        candidate = source.parent / value[len("$ORIGIN/") :]
    else:
        return None
    resolved = candidate.resolve(strict=False)
    return resolved if inside_engine_root(resolved) else None


def safe_existing_linux_rpaths(source: Path, raw: str) -> list[str]:
    result: list[str] = []
    for entry in raw.split(":"):
        entry = entry.strip().replace("${ORIGIN}", "$ORIGIN")
        if not entry:
            continue
        if expand_origin(source, entry) is not None and entry not in result:
            result.append(entry)
    return result


def resolve_linux_needed(
    source: Path,
    needed: str,
    candidates: list[Path],
    existing_rpaths: list[str],
) -> Path | None:
    if not candidates:
        return None

    # Preserve the wheel/plugin author's original intent first. This is the
    # critical rule for Pillow *.libs, pikepdf.libs, Graphviz plugins, etc.
    for entry in existing_rpaths:
        directory = expand_origin(source, entry)
        if directory is None:
            continue
        exact = directory / needed
        if exact.is_file() and exact in candidates:
            return exact

    local = source.parent / needed
    if local.is_file() and local in candidates:
        return local

    # LibreOffice is deliberately isolated from Conda.  Bind every external
    # Ubuntu dependency to its private distro closure before considering the
    # Conda runtime fallback.
    office_root = ENGINE_ROOT / "share" / "libreoffice"
    try:
        source.relative_to(office_root)
        in_office = True
    except ValueError:
        in_office = False
    if in_office:
        office_lib = office_root / "lib" / needed
        if office_lib.is_file() and office_lib in candidates:
            return office_lib

    # Conda's canonical shared-library directory is a deterministic fallback
    # for non-LibreOffice engines.
    runtime_lib = ENGINE_ROOT / "share" / "runtime" / "lib" / needed
    if runtime_lib.is_file() and runtime_lib in candidates:
        return runtime_lib

    if len(candidates) == 1:
        return candidates[0]

    rendered = ", ".join(str(p.relative_to(ENGINE_ROOT)) for p in candidates[:8])
    raise SystemExit(
        f"ambiguous internal ELF dependency for {source.relative_to(ENGINE_ROOT)}: "
        f"{needed} -> {rendered}"
    )


def linux_harden() -> None:
    patchelf = shutil.which("patchelf")
    readelf = shutil.which("readelf")
    if not patchelf or not readelf:
        raise SystemExit("Linux hardening requires patchelf and readelf")

    native = internal_native_files("linux")
    by_name: dict[str, list[Path]] = {}
    for path in native:
        by_name.setdefault(path.name, []).append(path)

    patched = 0
    for path in native:
        dynamic = run(readelf, "-d", str(path), check=False)
        if dynamic.returncode != 0 or "There is no dynamic section" in dynamic.stdout:
            continue

        needed_result = run(patchelf, "--print-needed", str(path), check=False)
        if needed_result.returncode != 0:
            raise SystemExit(f"patchelf --print-needed failed for {path}: {needed_result.stdout}")
        rpath_result = run(patchelf, "--print-rpath", str(path), check=False)
        old_rpath = rpath_result.stdout.strip() if rpath_result.returncode == 0 else ""
        entries = safe_existing_linux_rpaths(path, old_rpath)

        for needed in [line.strip() for line in needed_result.stdout.splitlines() if line.strip()]:
            target = resolve_linux_needed(path, needed, by_name.get(Path(needed).name, []), entries)
            if target is None:
                continue  # system dependency; closure validation handles it later
            derived = origin_to(path, target.parent)
            if expand_origin(path, derived) is None:
                raise SystemExit(f"refusing RPATH outside engine pack for {path}: {derived}")
            if derived not in entries:
                entries.append(derived)

        # Write only pack-relative entries.  If the original RPATH contained
        # only build-host paths, remove it completely instead of leaving Conda
        # / feedstock absolute paths embedded in the certified pack.
        if entries:
            result = run(patchelf, "--set-rpath", ":".join(entries), str(path), check=False)
            if result.returncode != 0:
                raise SystemExit(f"patchelf failed for {path}: {result.stdout}")
            patched += 1
        elif old_rpath:
            result = run(patchelf, "--remove-rpath", str(path), check=False)
            if result.returncode != 0:
                raise SystemExit(f"patchelf --remove-rpath failed for {path}: {result.stdout}")
            patched += 1

    print(f"[hardening] Linux dependency-aware relocation patched {patched}/{len(native)} ELF file(s)")


def macos_rpaths(path: Path, otool: str) -> list[str]:
    output = run(otool, "-l", str(path), check=False)
    if output.returncode != 0:
        return []
    lines = output.stdout.splitlines()
    result: list[str] = []
    for index, line in enumerate(lines):
        if line.strip() != "cmd LC_RPATH":
            continue
        for candidate in lines[index + 1 : index + 5]:
            match = re.match(r"\s*path\s+(.+?)\s+\(offset\s+\d+\)", candidate)
            if match:
                value = match.group(1).strip()
                if value not in result:
                    result.append(value)
                break
    return result


def expand_macos_path(value: str, source: Path) -> Path | None:
    if value == "@loader_path":
        return source.parent.resolve(strict=False)
    if value.startswith("@loader_path/"):
        return (source.parent / value[len("@loader_path/") :]).resolve(strict=False)
    if value == "@executable_path":
        return source.parent.resolve(strict=False)
    if value.startswith("@executable_path/"):
        # For relocation we only use this as a contextual hint. If it does not
        # resolve, basename/rpath uniqueness below decides or rejects ambiguity.
        return (source.parent / value[len("@executable_path/") :]).resolve(strict=False)
    if value.startswith("/"):
        return Path(value).resolve(strict=False)
    return None



def macos_absolute_pack_candidate(dep: str, candidates: list[Path]) -> Path | None:
    path = dep.replace("\\", "/")
    office_marker = "/LibreOffice.app/Contents/"
    if office_marker in path:
        suffix = path.split(office_marker, 1)[1]
        exact = ENGINE_ROOT / "share" / "libreoffice" / "Contents" / suffix
        if exact.is_file() and exact in candidates:
            return exact
    for marker, destination in (
        ("/lib/", ENGINE_ROOT / "share" / "runtime" / "lib"),
        ("/bin/", ENGINE_ROOT / "share" / "runtime" / "bin"),
        ("/libexec/", ENGINE_ROOT / "share" / "runtime" / "libexec"),
    ):
        if marker in path:
            suffix = path.rsplit(marker, 1)[1]
            exact = destination / suffix
            if exact.is_file() and exact in candidates:
                return exact
    return None

def resolve_macos_dependency(
    source: Path,
    dep: str,
    candidates: list[Path],
    rpaths: list[str],
) -> Path | None:
    if dep.startswith(SYSTEM_MAC_PREFIXES):
        return None
    if not candidates:
        return None

    contextual: list[Path] = []
    if dep.startswith("/"):
        mapped = macos_absolute_pack_candidate(dep, candidates)
        if mapped is not None:
            contextual.append(mapped)
    if dep.startswith("@rpath/"):
        suffix = dep[len("@rpath/") :]
        for rpath in rpaths:
            base = expand_macos_path(rpath, source)
            if base is None:
                continue
            exact = (base / suffix).resolve(strict=False)
            if exact.is_file() and exact in candidates:
                contextual.append(exact)
    else:
        exact = expand_macos_path(dep, source)
        if exact is not None and exact.is_file() and exact in candidates:
            contextual.append(exact)

    contextual = list(dict.fromkeys(contextual))
    if len(contextual) == 1:
        return contextual[0]
    if len(contextual) > 1:
        raise SystemExit(
            f"ambiguous dyld resolution for {source.relative_to(ENGINE_ROOT)}: {dep} -> "
            + ", ".join(str(p.relative_to(ENGINE_ROOT)) for p in contextual)
        )

    local = source.parent / Path(dep).name
    if local.is_file() and local in candidates:
        return local
    if len(candidates) == 1:
        return candidates[0]

    rendered = ", ".join(str(p.relative_to(ENGINE_ROOT)) for p in candidates[:8])
    raise SystemExit(
        f"ambiguous internal Mach-O dependency for {source.relative_to(ENGINE_ROOT)}: "
        f"{dep} -> {rendered}"
    )


def macos_harden(identity: str) -> None:
    install_name_tool = shutil.which("install_name_tool")
    codesign = shutil.which("codesign")
    otool = shutil.which("otool")
    if not install_name_tool or not otool:
        raise SystemExit("macOS hardening requires install_name_tool and otool")

    native = internal_native_files("macos")
    by_name: dict[str, list[Path]] = {}
    for path in native:
        by_name.setdefault(path.name, []).append(path)

    changed = 0
    for path in native:
        if path.suffix == ".dylib":
            run(install_name_tool, "-id", f"@rpath/{path.name}", str(path))

        rpaths = macos_rpaths(path, otool)
        deps = run(otool, "-L", str(path), check=False)
        if deps.returncode != 0:
            raise SystemExit(f"otool failed for {path}: {deps.stdout}")
        for line in deps.stdout.splitlines()[1:]:
            dep = line.strip().split(" (", 1)[0]
            if not dep or dep.startswith(SYSTEM_MAC_PREFIXES):
                continue
            target = resolve_macos_dependency(path, dep, by_name.get(Path(dep).name, []), rpaths)
            if target is None:
                # Unknown absolute non-system dependency is not portable and must
                # be rejected now rather than deferred to a later validator.
                if dep.startswith("/"):
                    raise SystemExit(f"non-portable unresolved Mach-O dependency for {path}: {dep}")
                continue
            replacement = loader_relative(path, target)
            if dep != replacement:
                run(install_name_tool, "-change", dep, replacement, str(path))
                changed += 1

    # All Mach-O mutations happen before this signing pass. Nothing in the
    # workflow is allowed to harden again after codesign/Authenticode.
    if identity:
        if not codesign:
            raise SystemExit("codesign is required for macOS engine signing")
        for path in sorted(native, key=lambda p: (-len(p.parts), str(p))):
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
    print(f"[hardening] macOS relocated {changed} dependency reference(s) across {len(native)} Mach-O file(s)")


def windows_harden() -> None:
    runtime = ENGINE_ROOT / "share" / "runtime"
    office = ENGINE_ROOT / "share" / "libreoffice" / "program"
    preferred = [
        BIN,
        runtime,
        runtime / "Library" / "bin",
        runtime / "Scripts",
        runtime / "DLLs",
        office,
    ]
    dll_dirs = {path.parent for path in ENGINE_ROOT.rglob("*.dll") if path.is_file()}
    ordered: list[Path] = []
    for path in [*preferred, *sorted(dll_dirs, key=lambda p: (len(p.parts), str(p).lower()))]:
        if path.is_dir() and inside_engine_root(path) and path not in ordered:
            ordered.append(path)

    relative = [path.relative_to(ENGINE_ROOT).as_posix() or "." for path in ordered]
    # Windows environment blocks are limited. A huge PATH is itself a broken
    # runtime, so fail during certification rather than on the user's machine.
    estimated = sum(len(item) + 1 for item in relative)
    if estimated > 24000:
        raise SystemExit(f"Windows private runtime PATH is too large ({estimated} characters)")
    WINDOWS_PATHS.write_text("\n".join(relative) + "\n", encoding="utf-8")
    print(f"[hardening] Windows private loader path contains {len(relative)} directory entries")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
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
    META.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--sign-identity", default=os.environ.get("APPLE_SIGNING_IDENTITY", ""))
    args = parser.parse_args()

    family = target_family(args.target)
    if family != host_family():
        raise SystemExit(f"engine hardening for {args.target} must run natively on {family}")

    if family == "macos":
        macos_harden(args.sign_identity.strip())
    elif family == "linux":
        linux_harden()
    else:
        windows_harden()
    refresh_metadata()


if __name__ == "__main__":
    main()
