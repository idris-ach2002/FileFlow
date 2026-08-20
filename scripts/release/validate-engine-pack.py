#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

from native_dependency_policy import (
    is_linux_system_dependency,
    is_macos_system_dependency,
    is_windows_system_dependency,
)

ROOT = Path(__file__).resolve().parents[2]
ENGINE_ROOT = ROOT / "src-tauri/resources/engines"
BIN = ENGINE_ROOT / "bin"
META = ROOT / "src-tauri/resources/engine-pack.json"
WINDOWS_PATHS = ENGINE_ROOT / "engine-runtime-paths.txt"


def is_linux_virtual_dependency(dep: str) -> bool:
    token = dep.strip().split(" (", 1)[0]
    return token in {"linux-vdso.so.1", "linux-gate.so.1"}


def clean_environment() -> dict[str, str]:
    """Environment representative of a user machine, not the build runner."""
    keep = (
        "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR", "TMP", "TEMP",
        "SystemRoot", "WINDIR", "USERPROFILE", "APPDATA", "LOCALAPPDATA",
    )
    env = {key: value for key in keep if (value := os.environ.get(key))}
    if os.name == "nt":
        system_root = env.get("SystemRoot", r"C:\Windows")
        env["PATH"] = os.pathsep.join([str(Path(system_root) / "System32"), system_root])
    else:
        env["PATH"] = "/usr/bin:/bin"
    env["PYTHONNOUSERSITE"] = "1"
    return env


def run(*args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=env,
    )


def files() -> list[Path]:
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


def file_output(path: Path) -> str:
    tool = shutil.which("file")
    return run(tool, "-b", str(path)).stdout if tool else ""


def expected_arch(target: str) -> tuple[str, ...]:
    if target.startswith("aarch64-"):
        return ("arm64", "aarch64", "ARM aarch64")
    if target.startswith("x86_64-"):
        return ("x86_64", "x86-64", "x86_64")
    return ()


def is_native(path: Path, family: str) -> bool:
    if family == "windows":
        return path.suffix.lower() in {".exe", ".dll"} and pe_headers(path) is not None
    kind = file_output(path)
    return "Mach-O" in kind if family == "macos" else "ELF" in kind


def inside_engine_root(path: Path) -> bool:
    try:
        path.resolve(strict=False).relative_to(ENGINE_ROOT.resolve())
        return True
    except ValueError:
        return False


def wrapper_targets(family: str) -> list[Path]:
    targets: list[Path] = []
    if family == "windows":
        for spec in BIN.glob("*.target"):
            lines = [line.strip() for line in spec.read_text(errors="replace").splitlines() if line.strip()]
            if not lines:
                continue
            value = lines[0]
            if value.startswith("{PACK}/"):
                target = ENGINE_ROOT / value[len("{PACK}/") :]
                if target.is_file():
                    targets.append(target)
        return targets

    pattern = re.compile(r'^TARGET="\$PACK_ROOT/(.+)"$', re.MULTILINE)
    for wrapper in BIN.iterdir() if BIN.is_dir() else []:
        if not wrapper.is_file():
            continue
        try:
            text = wrapper.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        match = pattern.search(text)
        if match:
            target = ENGINE_ROOT / match.group(1)
            if target.is_file():
                targets.append(target)
    return targets


def certification_seed(paths: list[Path], family: str) -> list[Path]:
    native = [p for p in paths if is_native(p, family)]
    # Wrapper targets may legitimately be scripts (Linux LibreOffice's
    # `soffice` is one). Only native targets belong in architecture/loader
    # closure validation; scripts remain covered by functional smoke tests.
    selected = {
        path for path in wrapper_targets(family)
        if is_native(path, family)
    }
    # These trees contain runtime-loaded modules not necessarily present in the
    # main executable's static dependency graph but required by declared engines.
    dynamic_hints = (
        "site-packages", "pillow.libs", "pikepdf.libs", "graphviz",
        "imagemagick", "ghostscript", "libreoffice", "vips-modules",
    )
    for path in native:
        rel = path.relative_to(ENGINE_ROOT).as_posix().lower()
        if any(hint in rel for hint in dynamic_hints):
            selected.add(path)
    return sorted(selected)


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


def linux_needed_and_rpaths(path: Path) -> tuple[list[str], list[str]]:
    patchelf = shutil.which("patchelf")
    if not patchelf:
        raise SystemExit("Linux validation requires patchelf")
    needed = run(patchelf, "--print-needed", str(path))
    if needed.returncode != 0:
        return [], []
    rpath = run(patchelf, "--print-rpath", str(path))
    entries = []
    if rpath.returncode == 0:
        entries = [entry.strip().replace("${ORIGIN}", "$ORIGIN") for entry in rpath.stdout.strip().split(":") if entry.strip()]
    return [line.strip() for line in needed.stdout.splitlines() if line.strip()], entries


def resolve_linux_internal(source: Path, needed: str, candidates: list[Path], rpaths: list[str]) -> Path | None:
    for entry in rpaths:
        base = expand_origin(source, entry)
        if base is None:
            continue
        exact = base / needed
        if exact.is_file() and exact in candidates:
            return exact
    local = source.parent / needed
    if local.is_file() and local in candidates:
        return local
    runtime = ENGINE_ROOT / "share" / "runtime" / "lib" / needed
    if runtime.is_file() and runtime in candidates:
        return runtime
    if len(candidates) == 1:
        return candidates[0]
    return None


def linux_closure(seed: list[Path], all_paths: list[Path]) -> list[Path]:
    native = [p for p in all_paths if is_native(p, "linux")]
    by_name: dict[str, list[Path]] = {}
    for path in native:
        by_name.setdefault(path.name, []).append(path)
    selected = set(seed)
    queue = list(seed)
    while queue:
        path = queue.pop()
        needed, rpaths = linux_needed_and_rpaths(path)
        for dep in needed:
            target = resolve_linux_internal(path, Path(dep).name, by_name.get(Path(dep).name, []), rpaths)
            if target is not None and target not in selected:
                selected.add(target)
                queue.append(target)
    return sorted(selected)


def macos_rpaths(path: Path) -> list[str]:
    output = run("otool", "-l", str(path))
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
                result.append(match.group(1).strip())
                break
    return result


def expand_macos(value: str, source: Path) -> Path | None:
    if value == "@loader_path":
        return source.parent.resolve(strict=False)
    if value.startswith("@loader_path/"):
        return (source.parent / value[len("@loader_path/") :]).resolve(strict=False)
    if value.startswith("@executable_path/"):
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

def resolve_macos_internal(source: Path, dep: str, candidates: list[Path]) -> Path | None:
    if is_macos_system_dependency(dep) or not candidates:
        return None
    hits: list[Path] = []
    if dep.startswith("/"):
        mapped = macos_absolute_pack_candidate(dep, candidates)
        if mapped is not None:
            hits.append(mapped)
    if dep.startswith("@rpath/"):
        suffix = dep[len("@rpath/") :]
        for entry in macos_rpaths(source):
            base = expand_macos(entry, source)
            if base is not None:
                exact = (base / suffix).resolve(strict=False)
                if exact.is_file() and exact in candidates:
                    hits.append(exact)
    else:
        exact = expand_macos(dep, source)
        if exact is not None and exact.is_file() and exact in candidates:
            hits.append(exact)
    hits = list(dict.fromkeys(hits))
    if len(hits) == 1:
        return hits[0]
    local = source.parent / Path(dep).name
    if local.is_file() and local in candidates:
        return local
    if len(candidates) == 1:
        return candidates[0]
    return None


def macos_closure(seed: list[Path], all_paths: list[Path]) -> list[Path]:
    native = [p for p in all_paths if is_native(p, "macos")]
    by_name: dict[str, list[Path]] = {}
    for path in native:
        by_name.setdefault(path.name, []).append(path)
    selected = set(seed)
    queue = list(seed)
    while queue:
        path = queue.pop()
        deps = run("otool", "-L", str(path))
        if deps.returncode != 0:
            continue
        for line in deps.stdout.splitlines()[1:]:
            dep = line.strip().split(" (", 1)[0]
            target = resolve_macos_internal(path, dep, by_name.get(Path(dep).name, []))
            if target is not None and target not in selected:
                selected.add(target)
                queue.append(target)
    return sorted(selected)


def pe_headers(path: Path) -> tuple[int, int, int, list[tuple[bytes, int, int, int]]] | None:
    try:
        data = path.read_bytes()
        if len(data) < 0x40 or data[:2] != b"MZ":
            return None
        pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
        if data[pe_offset : pe_offset + 4] != b"PE\0\0":
            return None
        coff = pe_offset + 4
        machine, sections, _, _, _, optional_size, _ = struct.unpack_from("<HHIIIHH", data, coff)
        optional = coff + 20
        section_offset = optional + optional_size
        table = []
        for index in range(sections):
            off = section_offset + index * 40
            if off + 40 > len(data):
                return None
            name = data[off : off + 8].rstrip(b"\0")
            virtual_size, virtual_address, raw_size, raw_pointer = struct.unpack_from("<IIII", data, off + 8)
            table.append((name, virtual_address, max(virtual_size, raw_size), raw_pointer))
        return machine, pe_offset, optional, table
    except (OSError, struct.error, IndexError):
        return None


def pe_machine(path: Path) -> int | None:
    headers = pe_headers(path)
    return headers[0] if headers else None


def pe_imports(path: Path) -> list[str]:
    try:
        data = path.read_bytes()
        headers = pe_headers(path)
        if not headers:
            return []
        _, _, optional, sections = headers
        magic = struct.unpack_from("<H", data, optional)[0]
        data_directory = optional + (112 if magic == 0x20B else 96 if magic == 0x10B else -1)
        if data_directory < optional:
            return []
        import_rva, import_size = struct.unpack_from("<II", data, data_directory + 8)
        if not import_rva or not import_size:
            return []

        def rva_to_offset(rva: int) -> int | None:
            for _, virtual_address, span, raw_pointer in sections:
                if virtual_address <= rva < virtual_address + span:
                    return raw_pointer + (rva - virtual_address)
            return None

        descriptor = rva_to_offset(import_rva)
        if descriptor is None:
            return []
        names = []
        for _ in range(4096):
            if descriptor + 20 > len(data):
                break
            original, timestamp, chain, name_rva, thunk = struct.unpack_from("<IIIII", data, descriptor)
            if original == timestamp == chain == name_rva == thunk == 0:
                break
            name_offset = rva_to_offset(name_rva)
            if name_offset is None:
                break
            end = data.find(b"\0", name_offset)
            if end < 0:
                break
            names.append(data[name_offset:end].decode("ascii", "replace"))
            descriptor += 20
        return names
    except (OSError, struct.error, IndexError):
        return []


def windows_search_dirs() -> list[Path]:
    if not WINDOWS_PATHS.is_file():
        return []
    result: list[Path] = []
    for line in WINDOWS_PATHS.read_text(encoding="utf-8").splitlines():
        value = line.strip()
        if not value:
            continue
        path = (ENGINE_ROOT / value).resolve(strict=False)
        if inside_engine_root(path) and path.is_dir() and path not in result:
            result.append(path)
    return result


def resolve_windows_dll(name: str, dirs: list[Path], source: Path | None = None) -> Path | None:
    lowered = name.lower()
    ordered = list(dirs)
    if source is not None:
        office = ENGINE_ROOT / "share" / "libreoffice"
        runtime = ENGINE_ROOT / "share" / "runtime"
        vendor = ENGINE_ROOT / "share" / "vendor" / "windows"
        try:
            source.relative_to(office)
            namespace = "office"
        except ValueError:
            try:
                source.relative_to(runtime)
                namespace = "runtime"
            except ValueError:
                namespace = "generic"

        def priority(directory: Path) -> tuple[int, int, str]:
            if namespace == "office" and inside_engine_root(directory):
                try:
                    directory.relative_to(office)
                    return (0, len(directory.parts), str(directory).lower())
                except ValueError:
                    pass
            if namespace == "runtime" and inside_engine_root(directory):
                try:
                    directory.relative_to(runtime)
                    return (0, len(directory.parts), str(directory).lower())
                except ValueError:
                    pass
            if inside_engine_root(directory):
                try:
                    directory.relative_to(vendor)
                    return (1, len(directory.parts), str(directory).lower())
                except ValueError:
                    pass
            return (2, len(directory.parts), str(directory).lower())

        ordered.sort(key=priority)

    for directory in ordered:
        try:
            for candidate in directory.iterdir():
                if candidate.is_file() and candidate.name.lower() == lowered:
                    return candidate
        except OSError:
            continue
    return None


def windows_closure(seed: list[Path], all_paths: list[Path]) -> list[Path]:
    dirs = windows_search_dirs()
    selected = set(seed)
    queue = list(seed)
    while queue:
        path = queue.pop()
        for dep in pe_imports(path):
            name = dep.lower()
            if is_windows_system_dependency(name):
                continue
            target = resolve_windows_dll(dep, dirs, path)
            if target is not None and target not in selected:
                selected.add(target)
                queue.append(target)
    return sorted(selected)


def selected_paths(all_paths: list[Path], family: str, scope: str) -> list[Path]:
    native = [p for p in all_paths if is_native(p, family)]
    if scope == "full":
        return native
    seed = certification_seed(all_paths, family)
    if family == "linux":
        return linux_closure(seed, all_paths)
    if family == "macos":
        return macos_closure(seed, all_paths)
    return windows_closure(seed, all_paths)


def validate_macos(paths: list[Path], target: str, require_signature: bool) -> list[str]:
    failures: list[str] = []
    expected = expected_arch(target)
    native_all = [p for p in files() if is_native(p, "macos")]
    by_name: dict[str, list[Path]] = {}
    for item in native_all:
        by_name.setdefault(item.name, []).append(item)
    for path in paths:
        info = file_output(path)
        if expected and not any(token in info for token in expected):
            failures.append(f"{path}: wrong architecture: {info.strip()}")
        deps = run("otool", "-L", str(path))
        if deps.returncode != 0:
            failures.append(f"{path}: otool failed: {deps.stdout.strip()}")
            continue
        for line in deps.stdout.splitlines()[1:]:
            dep = line.strip().split(" (", 1)[0]
            if is_macos_system_dependency(dep):
                continue
            candidates = by_name.get(Path(dep).name, [])
            resolved = resolve_macos_internal(path, dep, candidates)
            if resolved is None:
                failures.append(f"{path}: non-system Mach-O dependency is not bundled/reachable: {dep}")
        if require_signature:
            result = run("codesign", "--verify", "--strict", "--verbose=2", str(path))
            if result.returncode != 0:
                failures.append(f"{path}: invalid code signature: {result.stdout.strip()}")
    return failures


def parse_ldd_unresolved(output: str) -> list[str]:
    result: list[str] = []
    for line in output.splitlines():
        match = re.match(r"\s*([^\s]+)\s+=>\s+not found\s*$", line)
        if match and not is_linux_virtual_dependency(match.group(1)):
            result.append(match.group(1))
    return result


def validate_linux(paths: list[Path], target: str) -> list[str]:
    failures: list[str] = []
    expected = expected_arch(target)
    native_all = [p for p in files() if is_native(p, "linux")]
    by_name: dict[str, list[Path]] = {}
    for item in native_all:
        by_name.setdefault(item.name, []).append(item)
    env = clean_environment()

    for path in paths:
        info = file_output(path)
        if expected and not any(token in info for token in expected):
            failures.append(f"{path}: wrong architecture: {info.strip()}")
        needed, rpaths = linux_needed_and_rpaths(path)
        for entry in rpaths:
            if expand_origin(path, entry) is None:
                failures.append(f"{path}: non-portable RPATH/RUNPATH entry: {entry}")
        for dep in needed:
            candidates = by_name.get(Path(dep).name, [])
            if candidates:
                resolved = resolve_linux_internal(path, Path(dep).name, candidates, rpaths)
                if resolved is None:
                    failures.append(f"{path}: bundled dependency is unreachable via $ORIGIN RPATH: {dep}")
        ldd = run("ldd", str(path), env=env)
        if needed and ldd.returncode != 0:
            failures.append(f"{path}: ldd failed for dynamic ELF: {ldd.stdout.strip()}")
        elif ldd.returncode == 0:
            unresolved = parse_ldd_unresolved(ldd.stdout)
            if unresolved:
                failures.append(f"{path}: unresolved shared libraries: {', '.join(unresolved)}\n{ldd.stdout.strip()}")
            for line in ldd.stdout.splitlines():
                match = re.match(r"\s*([^\s]+)\s+=>\s+(/[^\s]+)", line)
                if not match:
                    continue
                soname, raw_resolved = match.groups()
                resolved = Path(raw_resolved).resolve(strict=False)
                if inside_engine_root(resolved):
                    continue
                if is_linux_system_dependency(soname):
                    continue
                failures.append(
                    f"{path}: non-baseline host dependency is not vendored: "
                    f"{soname} -> {resolved}"
                )
    return failures


def validate_windows(paths: list[Path], target: str, require_signature: bool) -> list[str]:
    failures: list[str] = []
    expected = 0x8664 if target.startswith("x86_64-") else 0xAA64
    dirs = windows_search_dirs()
    if not dirs:
        failures.append("Windows private loader path is missing; run harden-engine-pack.py before validation")
        return failures
    rendered_path_len = sum(len(str(path)) + 1 for path in dirs)
    if rendered_path_len > 28000:
        failures.append(f"Windows private loader PATH exceeds safe environment size: {rendered_path_len}")

    for path in paths:
        machine = pe_machine(path)
        if machine is None:
            continue
        if machine != expected:
            failures.append(f"{path}: PE machine 0x{machine:04x}, expected 0x{expected:04x}")
        for dependency in pe_imports(path):
            name = dependency.lower()
            if is_windows_system_dependency(name):
                continue
            resolved = resolve_windows_dll(dependency, dirs, path)
            if resolved is None:
                failures.append(
                    f"{path}: non-system imported DLL is not bundled/reachable through FileFlow private loader PATH: {dependency}"
                )
            else:
                dependency_machine = pe_machine(resolved)
                if dependency_machine is not None and dependency_machine != expected:
                    failures.append(f"{path}: imported DLL has wrong architecture: {dependency} -> {resolved}")
        if require_signature:
            escaped = str(path).replace("'", "''")
            cmd = f"$s=Get-AuthenticodeSignature -LiteralPath '{escaped}'; if ($s.Status -ne 'Valid') {{ Write-Error $s.Status; exit 2 }}"
            result = run("powershell", "-NoProfile", "-Command", cmd)
            if result.returncode != 0:
                failures.append(f"{path}: invalid Authenticode signature: {result.stdout.strip()}")
    return failures


def validate(paths: list[Path], family: str, target: str, require_signature: bool) -> list[str]:
    if family == "macos":
        return validate_macos(paths, target, require_signature)
    if family == "windows":
        return validate_windows(paths, target, require_signature)
    return validate_linux(paths, target)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--require-signature", action="store_true")
    parser.add_argument("--scope", choices=["certified", "full"], default="certified")
    parser.add_argument("--audit-full", action="store_true", help="report non-certified runtime issues without blocking")
    args = parser.parse_args()

    if not META.is_file():
        raise SystemExit("missing staged engine metadata")
    meta = json.loads(META.read_text())
    if meta.get("target") != args.target:
        raise SystemExit("staged engine metadata target mismatch")
    staged = [BIN / item["name"] for item in meta.get("engines", [])]
    for path in staged:
        if not path.is_file():
            raise SystemExit(f"missing staged engine: {path}")

    all_paths = files()
    family = "macos" if "apple-darwin" in args.target else ("windows" if "windows" in args.target else "linux")
    paths = selected_paths(all_paths, family, args.scope)
    failures = validate(paths, family, args.target, args.require_signature)
    if failures:
        print(f"native engine certification failed ({args.scope} scope):")
        for failure in failures:
            print("  -", failure)
        raise SystemExit(2)
    print(f"validated {len(paths)} native engine file(s) for {args.target} ({args.scope} scope)")

    if args.audit_full and args.scope != "full":
        certified = set(paths)
        extra = [p for p in selected_paths(all_paths, family, "full") if p not in certified]
        warnings = validate(extra, family, args.target, False) if extra else []
        if warnings:
            print(f"non-blocking full-runtime audit found {len(warnings)} issue(s):")
            for warning in warnings[:100]:
                print("  !", warning)
            if len(warnings) > 100:
                print(f"  ! ... {len(warnings) - 100} additional issue(s)")
        else:
            print(f"non-blocking full-runtime audit clean ({len(extra)} additional native file(s))")


if __name__ == "__main__":
    main()
