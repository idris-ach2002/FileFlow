#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import shutil
import stat
import subprocess
import sys
from pathlib import Path

from native_dependency_policy import (
    elf_machine,
    expected_elf_machine,
    expected_pe_machine,
    is_linux_system_dependency,
    is_macos_system_dependency,
    is_windows_system_dependency,
    pe_imports,
    pe_machine,
)

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "release/engines/manifest.json"
PACK_ROOT = ROOT / "release/engines/packs"
LIBREOFFICE_VENDOR_ROOT = ROOT / "release/engines/vendor/libreoffice"

DIRECT = {
    "ffmpeg": ["ffmpeg"],
    "ffprobe": ["ffprobe"],
    "magick": ["magick"],
    "vips": ["vips"],
    "qpdf": ["qpdf"],
    "7zz": ["7zz", "7z"],
    "zstd": ["zstd"],
    "lz4": ["lz4"],
    "tesseract": ["tesseract"],
    "pdftoppm": ["pdftoppm"],
    "pdftotext": ["pdftotext"],
    "gs": ["gs", "gswin64c", "gswin32c"],
    "pandoc": ["pandoc"],
}

PYTHON_MODULES = {
    "ocrmypdf": "ocrmypdf",
    "img2pdf": "img2pdf",
}

def log(message: str) -> None:
    print(f"[engine-factory] {message}", flush=True)


def run(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    log("+ " + " ".join(args))
    result = subprocess.run(
        args,
        cwd=str(cwd) if cwd else None,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if result.stdout:
        print(result.stdout, end="")
    return result


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def copytree(src: Path, dst: Path) -> None:
    if not src.exists():
        return
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst, symlinks=False)


def copy_runtime(prefix: Path, runtime: Path) -> None:
    if runtime.exists():
        shutil.rmtree(runtime)
    runtime.mkdir(parents=True)

    if os.name == "nt":
        for name in ("DLLs", "Lib", "Library", "Scripts", "share", "etc"):
            copytree(prefix / name, runtime / name)
        for pattern in ("python*.exe", "python*.dll", "vcruntime*.dll"):
            for item in prefix.glob(pattern):
                if item.is_file():
                    shutil.copy2(item, runtime / item.name)
    else:
        for name in ("bin", "lib", "libexec", "share", "etc"):
            copytree(prefix / name, runtime / name)

    # Development/cache material is not required at runtime.
    for relative in (
        "conda-meta",
        "include",
        "Library/include",
        "Library/lib/cmake",
        "lib/cmake",
        "lib/pkgconfig",
        "share/man",
        "share/doc",
        "share/info",
    ):
        candidate = runtime / relative
        if candidate.is_dir():
            shutil.rmtree(candidate, ignore_errors=True)

    for item in list(runtime.rglob("*")):
        if item.is_dir() and item.name == "__pycache__":
            shutil.rmtree(item, ignore_errors=True)
        elif item.is_file() and item.suffix in {".a", ".la", ".pyc"}:
            item.unlink(missing_ok=True)


def executable_variants(name: str) -> list[str]:
    if os.name == "nt":
        return [name if name.lower().endswith(".exe") else f"{name}.exe"]
    return [name]


def find_runtime_program(runtime: Path, aliases: list[str]) -> Path:
    roots = (
        [runtime, runtime / "Library" / "bin", runtime / "Scripts", runtime / "bin"]
        if os.name == "nt"
        else [runtime / "bin"]
    )

    for alias in aliases:
        for root in roots:
            for variant in executable_variants(alias):
                candidate = root / variant
                if candidate.is_file():
                    return candidate

    lowered = {
        variant.lower()
        for alias in aliases
        for variant in executable_variants(alias)
    }
    matches = [
        path
        for path in runtime.rglob("*")
        if path.is_file() and path.name.lower() in lowered
    ]
    if matches:
        return sorted(matches, key=lambda p: (len(p.parts), str(p)))[0]

    raise SystemExit(
        f"engine factory could not locate any of {aliases} below {runtime}"
    )


def find_python(runtime: Path) -> Path:
    candidates: list[Path] = []
    if os.name == "nt":
        candidates.append(runtime / "python.exe")
    else:
        candidates.extend([runtime / "bin" / "python", runtime / "bin" / "python3"])
        if (runtime / "bin").is_dir():
            candidates.extend(sorted((runtime / "bin").glob("python3.*")))
    for path in candidates:
        if path.is_file():
            return path
    raise SystemExit("bundled Python runtime is missing")


def find_perl(runtime: Path) -> Path | None:
    candidates = (
        [runtime / "Library" / "bin" / "perl.exe", runtime / "perl.exe"]
        if os.name == "nt"
        else [runtime / "bin" / "perl"]
    )
    return next((path for path in candidates if path.is_file()), None)


def find_exiftool(runtime: Path, pack: Path) -> tuple[Path, list[str]]:
    names = (
        ["exiftool.exe", "exiftool", "exiftool.pl"]
        if os.name == "nt"
        else ["exiftool"]
    )
    for name in names:
        matches = sorted(path for path in runtime.rglob(name) if path.is_file())
        for path in matches:
            if os.name == "nt" and path.suffix.lower() == ".exe":
                return path, []
            perl = find_perl(runtime)
            if perl is not None:
                rel = path.relative_to(pack).as_posix()
                return perl, [f"{{PACK}}/{rel}"]
            return path, []
    raise SystemExit("engine factory could not locate exiftool")


def linux_needed(path: Path) -> list[str]:
    patchelf = shutil.which("patchelf")
    if not patchelf:
        raise SystemExit("Linux engine factory requires patchelf")
    result = subprocess.run(
        [patchelf, "--print-needed", str(path)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        return []
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def linux_elf(path: Path) -> bool:
    try:
        with path.open("rb") as handle:
            return handle.read(4) == b"\x7fELF"
    except OSError:
        return False


def linux_host_library_index() -> dict[str, list[Path]]:
    result: dict[str, list[Path]] = {}
    ldconfig = shutil.which("ldconfig")
    if ldconfig:
        proc = subprocess.run(
            [ldconfig, "-p"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        if proc.returncode == 0:
            for line in proc.stdout.splitlines():
                if "=>" not in line:
                    continue
                left, raw_path = line.rsplit("=>", 1)
                soname = left.strip().split()[0] if left.strip() else ""
                path = Path(raw_path.strip())
                if soname and path.is_file():
                    result.setdefault(soname, []).append(path)
    return result


def find_linux_host_library(
    soname: str,
    index: dict[str, list[Path]],
    target: str,
) -> Path | None:
    expected_machine = expected_elf_machine(target)

    def usable(path: Path) -> bool:
        if not path.is_file():
            return False
        machine = elf_machine(path.resolve(strict=False))
        return machine is not None and (expected_machine is None or machine == expected_machine)

    candidates = [path for path in index.get(soname, []) if usable(path)]
    if candidates:
        return sorted(candidates, key=lambda path: (len(path.parts), str(path)))[0]

    # Some optional Ubuntu libraries (notably lp_solve) are installed in a
    # subdirectory that is not necessarily present in ldconfig's cache.
    for root in (Path("/usr/lib"), Path("/lib")):
        if not root.is_dir():
            continue
        matches = sorted(root.rglob(soname), key=lambda path: (len(path.parts), str(path)))
        for path in matches:
            if usable(path):
                return path
    return None


def _path_is_below(path: Path, root: Path) -> bool:
    try:
        path.resolve(strict=False).relative_to(root.resolve(strict=False))
        return True
    except ValueError:
        return False


def _vendor_record(
    family: str,
    dependency: str,
    source: Path,
    destination: Path,
    pack: Path,
) -> dict[str, object]:
    return {
        "family": family,
        "dependency": dependency,
        "source": str(source),
        "destination": destination.relative_to(pack).as_posix(),
        "sha256": digest(destination),
        "size": destination.stat().st_size,
    }


def vendor_linux_external_dependencies(pack: Path, target: str) -> list[dict[str, object]]:
    """Vendor every non-baseline Linux DT_NEEDED dependency missing from pack.

    LibreOffice is kept in an isolated distro namespace because a SONAME can
    exist in both Ubuntu and Conda with incompatible build options. Other
    missing host dependencies are copied into share/vendor/linux. Provider
    selection is architecture checked, preventing accidental i386 libraries on
    the x86_64 runner (the exact class of failure seen in CI).
    """
    office = pack / "share" / "libreoffice"
    office_lib = office / "lib"
    office_lib.mkdir(parents=True, exist_ok=True)
    vendor_lib = pack / "share" / "vendor" / "linux"
    vendor_lib.mkdir(parents=True, exist_ok=True)

    index = linux_host_library_index()
    expected_machine = expected_elf_machine(target)

    def native(path: Path) -> bool:
        machine = elf_machine(path)
        return machine is not None and (expected_machine is None or machine == expected_machine)

    def rebuild_index() -> dict[str, list[Path]]:
        result: dict[str, list[Path]] = {}
        for path in pack.rglob("*"):
            if path.is_file() and native(path):
                result.setdefault(path.name, []).append(path)
        return result

    by_name = rebuild_index()
    queue = [path for paths in by_name.values() for path in paths]
    scanned: set[Path] = set()
    records: list[dict[str, object]] = []

    while queue:
        source = queue.pop()
        resolved_source = source.resolve(strict=False)
        if resolved_source in scanned:
            continue
        scanned.add(resolved_source)
        for soname in linux_needed(source):
            if is_linux_system_dependency(soname):
                continue

            in_office = _path_is_below(source, office)
            candidates = by_name.get(soname, [])
            if in_office:
                # LibreOffice may not bind to a Conda duplicate with the same
                # SONAME. Any same-architecture copy inside its own tree is OK.
                if any(_path_is_below(candidate, office) for candidate in candidates):
                    continue
                destination_root = office_lib
            else:
                if any(not _path_is_below(candidate, office) for candidate in candidates):
                    continue
                destination_root = vendor_lib

            host = find_linux_host_library(soname, index, target)
            if host is None:
                raise SystemExit(
                    "Linux dependency is unavailable for the target architecture: "
                    f"{source.relative_to(pack)} -> {soname}. "
                    "Install the providing package in the native engine job."
                )

            destination = destination_root / soname
            if destination.exists():
                if not native(destination):
                    destination.unlink()
                else:
                    by_name.setdefault(soname, []).append(destination)
                    continue
            shutil.copy2(host.resolve(), destination)
            if not native(destination):
                machine = elf_machine(destination)
                destination.unlink(missing_ok=True)
                raise SystemExit(
                    f"refusing wrong-architecture Linux provider for {soname}: "
                    f"{host} (ELF machine={machine}, target={target})"
                )
            by_name.setdefault(soname, []).append(destination)
            queue.append(destination)
            records.append(
                _vendor_record("linux", soname, host, destination, pack)
            )
            namespace = "libreoffice" if in_office else "vendor"
            log(f"vendored Linux {namespace} dependency {soname} <- {host}")

    log(
        "Linux dependency closure complete: "
        f"vendored={len(records)} office={sum(1 for r in records if str(r['destination']).startswith('share/libreoffice/'))} "
        f"generic={sum(1 for r in records if str(r['destination']).startswith('share/vendor/'))}"
    )
    return records


def vendor_linux_libreoffice_dependencies(pack: Path, target: str | None = None) -> None:
    """Backward-compatible test/helper entry point for the Linux closure."""
    if target is None:
        machine = next(
            (elf_machine(path) for path in (pack / "share" / "libreoffice").rglob("*") if path.is_file() and linux_elf(path)),
            None,
        )
        target = "aarch64-unknown-linux-gnu" if machine == 183 else "x86_64-unknown-linux-gnu"
    vendor_linux_external_dependencies(pack, target)


def macos_dependencies(path: Path) -> list[str]:
    otool = shutil.which("otool")
    if not otool:
        raise SystemExit("macOS engine factory requires otool")
    result = subprocess.run(
        [otool, "-L", str(path)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if result.returncode != 0:
        return []
    return [
        line.strip().split(" (", 1)[0]
        for line in result.stdout.splitlines()[1:]
        if line.strip()
    ]


def macos_rpaths(path: Path) -> list[str]:
    otool = shutil.which("otool")
    if not otool:
        return []
    result = subprocess.run(
        [otool, "-l", str(path)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if result.returncode != 0:
        return []
    lines = result.stdout.splitlines()
    values: list[str] = []
    for index, line in enumerate(lines):
        if line.strip() != "cmd LC_RPATH":
            continue
        for candidate in lines[index + 1 : index + 5]:
            if " path " not in f" {candidate.strip()} ":
                continue
            value = candidate.strip().split("path ", 1)[1].split(" (offset", 1)[0].strip()
            if value and value not in values:
                values.append(value)
            break
    return values


def macos_is_native(path: Path, target: str) -> bool:
    tool = shutil.which("lipo")
    if not tool or not path.is_file():
        return False
    result = subprocess.run(
        [tool, "-archs", str(path)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        return False
    required = "arm64" if target.startswith("aarch64-") else "x86_64"
    return required in result.stdout.split()


def _expand_macos_host_path(source: Path, value: str) -> Path | None:
    if value == "@loader_path":
        return source.parent
    if value.startswith("@loader_path/"):
        return source.parent / value[len("@loader_path/") :]
    if value == "@executable_path":
        return source.parent
    if value.startswith("@executable_path/"):
        return source.parent / value[len("@executable_path/") :]
    if value.startswith("/"):
        return Path(value)
    return None


def find_macos_host_dependency(source: Path, dependency: str) -> Path | None:
    if dependency.startswith("/"):
        candidate = Path(dependency)
        return candidate if candidate.is_file() else None
    if dependency.startswith("@rpath/"):
        suffix = dependency[len("@rpath/") :]
        for entry in macos_rpaths(source):
            base = _expand_macos_host_path(source, entry)
            if base is not None:
                candidate = (base / suffix).resolve(strict=False)
                if candidate.is_file():
                    return candidate
        return None
    candidate = _expand_macos_host_path(source, dependency)
    return candidate.resolve(strict=False) if candidate is not None and candidate.is_file() else None


def vendor_macos_external_dependencies(pack: Path, target: str, prefix: Path) -> list[dict[str, object]]:
    office = pack / "share" / "libreoffice"
    office_vendor = office / "lib"
    generic_vendor = pack / "share" / "vendor" / "macos"
    office_vendor.mkdir(parents=True, exist_ok=True)
    generic_vendor.mkdir(parents=True, exist_ok=True)

    def native_paths() -> list[Path]:
        return [path for path in pack.rglob("*") if path.is_file() and macos_is_native(path, target)]

    by_name: dict[str, list[Path]] = {}
    queue = native_paths()
    for path in queue:
        by_name.setdefault(path.name, []).append(path)
    scanned: set[Path] = set()
    records: list[dict[str, object]] = []

    while queue:
        source = queue.pop()
        resolved_source = source.resolve(strict=False)
        if resolved_source in scanned:
            continue
        scanned.add(resolved_source)
        for dep in macos_dependencies(source):
            if is_macos_system_dependency(dep):
                continue
            name = Path(dep).name
            in_office = _path_is_below(source, office)

            # Exact copied trees are preferred over basename heuristics.
            if dep.startswith("/Applications/LibreOffice.app/Contents/"):
                exact = office / "Contents" / dep.split("/Applications/LibreOffice.app/Contents/", 1)[1]
                if exact.is_file() and macos_is_native(exact, target):
                    continue
            if dep.startswith(str(prefix) + "/"):
                exact = pack / "share" / "runtime" / Path(dep).relative_to(prefix)
                if exact.is_file() and macos_is_native(exact, target):
                    continue
            if dep.startswith(("@loader_path", "@executable_path")):
                exact = _expand_macos_host_path(source, dep)
                if exact is not None and exact.is_file() and _path_is_below(exact, pack):
                    continue

            candidates = by_name.get(name, [])
            if in_office:
                if any(_path_is_below(candidate, office) for candidate in candidates):
                    continue
                destination_root = office_vendor
            else:
                if candidates:
                    continue
                destination_root = generic_vendor

            host = find_macos_host_dependency(source, dep)
            if host is None or _path_is_below(host, pack):
                raise SystemExit(
                    "macOS dependency is not represented inside FileFlow and cannot be resolved on the build host: "
                    f"{source.relative_to(pack)} -> {dep}"
                )
            if not macos_is_native(host, target):
                raise SystemExit(f"refusing wrong-architecture macOS provider for {dep}: {host}")
            destination = destination_root / name
            if destination.exists() and digest(destination) != digest(host):
                raise SystemExit(f"conflicting macOS dependency providers for {name}: {host} vs {destination}")
            if not destination.exists():
                shutil.copy2(host, destination)
                by_name.setdefault(name, []).append(destination)
                queue.append(destination)
                records.append(_vendor_record("macos", dep, host, destination, pack))
                log(f"vendored macOS dependency {dep} <- {host}")

    log(f"macOS dependency closure complete: vendored={len(records)}")
    return records


def _windows_host_roots(prefix: Path) -> list[Path]:
    roots = [prefix]
    for variable in ("ProgramFiles", "ProgramFiles(x86)"):
        value = os.environ.get(variable)
        if value:
            office = Path(value) / "LibreOffice"
            if office.is_dir():
                roots.append(office)
    system_root = Path(os.environ.get("SystemRoot", r"C:\Windows")).resolve(strict=False)
    for raw in os.environ.get("PATH", "").split(os.pathsep):
        if not raw:
            continue
        path = Path(raw)
        try:
            resolved = path.resolve(strict=False)
            resolved.relative_to(system_root)
            continue
        except (ValueError, OSError):
            pass
        if path.is_dir() and path not in roots:
            roots.append(path)
    return roots


def windows_host_dll_index(prefix: Path, target: str) -> dict[str, list[Path]]:
    expected = expected_pe_machine(target)
    result: dict[str, list[Path]] = {}
    for root in _windows_host_roots(prefix):
        if not root.is_dir():
            continue
        recursive = root == prefix or root.name.lower() == "libreoffice"
        paths = root.rglob("*.dll") if recursive else root.glob("*.dll")
        for path in paths:
            if not path.is_file():
                continue
            machine = pe_machine(path)
            if machine is not None and (expected is None or machine == expected):
                result.setdefault(path.name.lower(), []).append(path)
    return result


def vendor_windows_external_dependencies(pack: Path, target: str, prefix: Path) -> list[dict[str, object]]:
    expected = expected_pe_machine(target)
    vendor = pack / "share" / "vendor" / "windows"
    vendor.mkdir(parents=True, exist_ok=True)
    office = pack / "share" / "libreoffice"
    runtime = pack / "share" / "runtime"

    def native(path: Path) -> bool:
        machine = pe_machine(path)
        return machine is not None and (expected is None or machine == expected)

    by_name: dict[str, list[Path]] = {}
    queue: list[Path] = []
    for path in pack.rglob("*"):
        if path.is_file() and native(path):
            by_name.setdefault(path.name.lower(), []).append(path)
            queue.append(path)

    host_index = windows_host_dll_index(prefix, target)
    office_host_roots = [
        Path(value) / "LibreOffice"
        for variable in ("ProgramFiles", "ProgramFiles(x86)")
        if (value := os.environ.get(variable))
    ]
    scanned: set[Path] = set()
    records: list[dict[str, object]] = []
    while queue:
        source = queue.pop()
        resolved_source = source.resolve(strict=False)
        if resolved_source in scanned:
            continue
        scanned.add(resolved_source)
        for dep in pe_imports(source):
            if is_windows_system_dependency(dep):
                continue
            name = dep.lower()
            candidates = by_name.get(name, [])
            if _path_is_below(source, office):
                if any(_path_is_below(candidate, office) for candidate in candidates):
                    continue
            elif _path_is_below(source, runtime):
                if any(_path_is_below(candidate, runtime) for candidate in candidates):
                    continue
            elif candidates:
                continue

            providers = host_index.get(name, [])
            if _path_is_below(source, office):
                providers = [
                    provider
                    for provider in providers
                    if any(_path_is_below(provider, root) for root in office_host_roots if root.is_dir())
                ]
            elif _path_is_below(source, runtime):
                providers = [provider for provider in providers if _path_is_below(provider, prefix)]
            if not providers:
                raise SystemExit(
                    "Windows imported DLL is not bundled and no controlled non-system provider was found: "
                    f"{source.relative_to(pack)} -> {dep}. "
                    "Install the providing package in the native engine job or explicitly classify it as a Windows system DLL."
                )
            provider = sorted(providers, key=lambda path: (len(path.parts), str(path).lower()))[0]
            destination = vendor / dep
            if destination.exists() and digest(destination) != digest(provider):
                raise SystemExit(f"conflicting Windows dependency providers for {dep}: {provider} vs {destination}")
            if not destination.exists():
                shutil.copy2(provider, destination)
                if not native(destination):
                    destination.unlink(missing_ok=True)
                    raise SystemExit(f"refusing wrong-architecture Windows provider for {dep}: {provider}")
                by_name.setdefault(name, []).append(destination)
                queue.append(destination)
                records.append(_vendor_record("windows", dep, provider, destination, pack))
                log(f"vendored Windows dependency {dep} <- {provider}")

    log(f"Windows dependency closure complete: vendored={len(records)}")
    return records


def install_libreoffice(pack: Path, target_triple: str) -> Path:
    """Copy the pinned upstream LibreOffice payload prepared by CI.

    Never copy a distro/Homebrew/Chocolatey installation here. Those installed
    trees can contain absolute /etc, /var, /usr/share or package-manager links
    and are not portable build inputs. fetch-libreoffice-runtime.py normalizes
    the official TDF artifact into a target-specific tree first.
    """
    source = LIBREOFFICE_VENDOR_ROOT / target_triple
    metadata = source / ".fileflow-source.json"
    if not source.is_dir() or not metadata.is_file():
        raise SystemExit(
            "official LibreOffice runtime is not prepared for "
            f"{target_triple}; run scripts/release/fetch-libreoffice-runtime.py "
            f"--target {target_triple}"
        )

    destination = pack / "share" / "libreoffice"
    if destination.exists():
        shutil.rmtree(destination)
    # Preserve the upstream layout and symlinks. Location-sensitive UNO/URE
    # components must remain in the exact relative structure shipped by TDF.
    shutil.copytree(source, destination, symlinks=True)

    if sys.platform.startswith("linux"):
        target = destination / "program" / "soffice"
    elif sys.platform == "darwin":
        target = destination / "Contents" / "MacOS" / "soffice"
    elif os.name == "nt":
        target = destination / "program" / "soffice.exe"
    else:
        raise SystemExit(f"unsupported host for LibreOffice: {sys.platform}")

    if not target.is_file():
        raise SystemExit(f"LibreOffice launcher missing after upstream copy: {target}")
    return target


def shell_arg(arg: str) -> str:
    if arg.startswith("{PACK}/"):
        relative = arg[len("{PACK}/") :].replace('"', '\\"')
        return f'"$PACK_ROOT/{relative}"'
    return shlex.quote(arg)


def unix_wrapper(target_rel: str, fixed_args: list[str] | None = None, *, office: bool = False) -> str:
    fixed_args = fixed_args or []
    fixed = " ".join(shell_arg(arg) for arg in fixed_args)
    target = target_rel.replace('"', '\\"')
    spacer = f" {fixed}" if fixed else ""
    office_mode = "1" if office else "0"
    return f'''#!/bin/sh
set -eu
BIN_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
PACK_ROOT="$(CDPATH= cd -- "$BIN_DIR/.." && pwd)"
RUNTIME="$PACK_ROOT/share/runtime"
VENDOR="$PACK_ROOT/share/vendor"
OFFICE_LIB="$PACK_ROOT/share/libreoffice/lib"

# Zero-dependency host contract: never inherit Conda/Python/library paths from
# the machine that happens to launch FileFlow. Engine-to-engine subprocesses
# resolve through FileFlow's own wrappers first.
unset CONDA_PREFIX CONDA_DEFAULT_ENV CONDA_EXE MAMBA_EXE PYTHONPATH LD_PRELOAD
export PYTHONNOUSERSITE=1
export PATH="$BIN_DIR:$RUNTIME/bin:$RUNTIME/Library/bin:$RUNTIME/Scripts:/usr/bin:/bin"
if [ "{office_mode}" = "1" ]; then
  # Preserve LibreOffice's upstream loader namespace. Injecting a broad
  # LD_LIBRARY_PATH changes resolution order for UNO/URE/plugin components and
  # can produce DeploymentException/abort failures. Linux/Mach-O hardening
  # gives each native object loader-relative access to any extra vendored lib.
  unset PYTHONHOME PYTHONPATH LD_LIBRARY_PATH DYLD_LIBRARY_PATH
  export SAL_USE_VCLPLUGIN=svp
else
  PRIVATE_LIBS=""
  for LIBDIR in "$RUNTIME/lib" "$VENDOR/linux" "$VENDOR/macos"; do
    if [ -d "$LIBDIR" ]; then
      if [ -z "$PRIVATE_LIBS" ]; then PRIVATE_LIBS="$LIBDIR"; else PRIVATE_LIBS="$PRIVATE_LIBS:$LIBDIR"; fi
    fi
  done
  [ -z "$PRIVATE_LIBS" ] || export LD_LIBRARY_PATH="$PRIVATE_LIBS"
  [ -z "$PRIVATE_LIBS" ] || export DYLD_LIBRARY_PATH="$PRIVATE_LIBS"
  export PYTHONHOME="$RUNTIME"
fi

for TESS in "$RUNTIME/share/tessdata" "$RUNTIME/Library/share/tessdata"; do
  if [ -d "$TESS" ]; then export TESSDATA_PREFIX="$TESS"; break; fi
done

export MAGICK_HOME="$RUNTIME"
for MAGICK in "$RUNTIME/etc/ImageMagick-"* "$RUNTIME/share/ImageMagick-"* "$RUNTIME/Library/etc/ImageMagick-"* "$RUNTIME/Library/share/ImageMagick-"*; do
  if [ -d "$MAGICK" ]; then export MAGICK_CONFIGURE_PATH="$MAGICK"; break; fi
done
for CODERS in "$RUNTIME/lib/ImageMagick-"*/modules-*/coders "$RUNTIME/Library/lib/ImageMagick-"*/modules-*/coders; do
  if [ -d "$CODERS" ]; then export MAGICK_CODER_MODULE_PATH="$CODERS"; break; fi
done

GS_PATHS=""
for GSDIR in "$RUNTIME/share/ghostscript/"*/Resource/Init "$RUNTIME/share/ghostscript/"*/lib "$RUNTIME/Library/share/ghostscript/"*/Resource/Init "$RUNTIME/Library/share/ghostscript/"*/lib; do
  if [ -d "$GSDIR" ]; then
    if [ -z "$GS_PATHS" ]; then GS_PATHS="$GSDIR"; else GS_PATHS="$GS_PATHS:$GSDIR"; fi
  fi
done
[ -z "$GS_PATHS" ] || export GS_LIB="$GS_PATHS"

TARGET="$PACK_ROOT/{target}"
exec "$TARGET"{spacer} "$@"
'''


WINDOWS_LAUNCHER = r'''use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn replace_pack(value: &str, pack: &Path) -> OsString {
    if let Some(rest) = value.strip_prefix("{PACK}/") {
        return pack.join(rest.replace('/', "\\")).into_os_string();
    }
    OsString::from(value)
}

fn configured_private_paths(pack: &Path, bin: &Path, office_mode: bool) -> Vec<PathBuf> {
    let mut configured: Vec<PathBuf> = Vec::new();
    let config = pack.join("engine-runtime-paths.txt");
    if let Ok(text) = fs::read_to_string(config) {
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let path = pack.join(line.replace('/', "\\"));
            if path.is_dir() && !configured.contains(&path) {
                configured.push(path);
            }
        }
    }
    if office_mode {
        configured.sort_by_key(|path| {
            let rendered = path.to_string_lossy().to_ascii_lowercase();
            if rendered.contains("share\\libreoffice") { 0 } else { 1 }
        });
    }
    let mut values = vec![bin.to_path_buf()];
    values.extend(configured);
    values
}

fn set_private_path(entries: &[PathBuf]) {
    let mut values: Vec<PathBuf> = entries.iter().filter(|p| p.is_dir()).cloned().collect();
    if let Some(root) = env::var_os("SystemRoot").map(PathBuf::from) {
        values.push(root.join("System32"));
        values.push(root);
    }
    if let Ok(joined) = env::join_paths(values) {
        env::set_var("PATH", joined);
    }
}

fn first_prefixed_dir(parent: &Path, prefix: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(parent).ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| path.file_name().and_then(OsStr::to_str).map(|name| name.starts_with(prefix)).unwrap_or(false))
        .collect();
    found.sort();
    found.into_iter().next()
}

fn first_named_dir_below(root: &Path, wanted: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !root.is_dir() { return None; }
    let mut entries: Vec<PathBuf> = fs::read_dir(root).ok()?.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    entries.sort();
    for path in &entries {
        if path.file_name().and_then(OsStr::to_str) == Some(wanted) { return Some(path.clone()); }
    }
    for path in entries {
        if let Some(found) = first_named_dir_below(&path, wanted, depth - 1) { return Some(found); }
    }
    None
}

fn configure_engine_environment(runtime: &Path) {
    env::remove_var("CONDA_PREFIX");
    env::remove_var("CONDA_DEFAULT_ENV");
    env::remove_var("CONDA_EXE");
    env::remove_var("MAMBA_EXE");
    env::remove_var("PYTHONPATH");
    env::set_var("PYTHONHOME", runtime);
    env::set_var("PYTHONNOUSERSITE", "1");

    for tess in [
        runtime.join("share").join("tessdata"),
        runtime.join("Library").join("share").join("tessdata"),
    ] {
        if tess.is_dir() {
            env::set_var("TESSDATA_PREFIX", tess);
            break;
        }
    }

    env::set_var("MAGICK_HOME", runtime);
    for parent in [runtime.join("etc"), runtime.join("share"), runtime.join("Library").join("etc"), runtime.join("Library").join("share")] {
        if let Some(path) = first_prefixed_dir(&parent, "ImageMagick-") {
            env::set_var("MAGICK_CONFIGURE_PATH", path);
            break;
        }
    }
    for parent in [runtime.join("lib"), runtime.join("Library").join("lib")] {
        if let Some(coders) = first_named_dir_below(&parent, "coders", 4) {
            env::set_var("MAGICK_CODER_MODULE_PATH", coders);
            break;
        }
    }

    let mut ghost_paths: Vec<PathBuf> = Vec::new();
    for ghost_root in [runtime.join("share").join("ghostscript"), runtime.join("Library").join("share").join("ghostscript")] {
        if let Ok(entries) = fs::read_dir(&ghost_root) {
            let mut versions: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
            versions.sort();
            if let Some(version) = versions.into_iter().last() {
                for candidate in [version.join("Resource").join("Init"), version.join("lib")] {
                    if candidate.is_dir() { ghost_paths.push(candidate); }
                }
            }
        }
    }
    if let Ok(joined) = env::join_paths(ghost_paths) {
        env::set_var("GS_LIB", joined);
    }
}

fn configure_office_environment() {
    for key in ["CONDA_PREFIX", "CONDA_DEFAULT_ENV", "CONDA_EXE", "MAMBA_EXE", "PYTHONHOME", "PYTHONPATH"] {
        env::remove_var(key);
    }
}

fn main() -> ExitCode {
    let me = match env::current_exe() {
        Ok(v) => v,
        Err(e) => { eprintln!("FileFlow engine launcher: current_exe: {e}"); return ExitCode::from(111); }
    };
    let bin = match me.parent() {
        Some(v) => v,
        None => return ExitCode::from(112),
    };
    let pack = match bin.parent() {
        Some(v) => v,
        None => return ExitCode::from(113),
    };
    let stem = me.file_stem().and_then(OsStr::to_str).unwrap_or("");
    let office_mode = stem.eq_ignore_ascii_case("soffice");
    let spec = bin.join(format!("{stem}.target"));
    let text = match fs::read_to_string(&spec) {
        Ok(v) => v,
        Err(e) => { eprintln!("FileFlow engine launcher: {}: {e}", spec.display()); return ExitCode::from(114); }
    };
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let target_line = match lines.next() {
        Some(v) => v.trim(),
        None => return ExitCode::from(115),
    };
    let target = if let Some(rest) = target_line.strip_prefix("{PACK}/") {
        pack.join(rest.replace('/', "\\"))
    } else {
        PathBuf::from(target_line)
    };

    let runtime = pack.join("share").join("runtime");
    let private_paths = configured_private_paths(pack, bin, office_mode);
    set_private_path(&private_paths);
    if office_mode {
        configure_office_environment();
    } else {
        configure_engine_environment(&runtime);
    }

    let mut command = Command::new(&target);
    for line in lines {
        command.arg(replace_pack(line.trim(), pack));
    }
    command.args(env::args_os().skip(1));

    match command.status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("FileFlow engine launcher: {}: {e}", target.display());
            ExitCode::from(116)
        }
    }
}
'''



def write_windows_launcher_source() -> Path:
    source = ROOT / "scripts" / "release" / "windows-engine-launcher.rs"
    source.write_text(WINDOWS_LAUNCHER, encoding="utf-8")
    return source


def build_windows_launcher(pack: Path) -> Path:
    source = write_windows_launcher_source()
    launcher = pack / ".engine-launcher.exe"
    run("rustc", "--edition=2021", "-O", str(source), "-o", str(launcher))
    if not launcher.is_file():
        raise SystemExit("rustc did not produce the Windows engine launcher")
    return launcher


def relative_to_pack(path: Path, pack: Path) -> str:
    return path.relative_to(pack).as_posix()


def add_unix_command(
    pack: Path,
    name: str,
    target: Path,
    fixed: list[str] | None = None,
    *,
    office: bool = False,
) -> None:
    path = pack / "bin" / name
    path.write_text(
        unix_wrapper(relative_to_pack(target, pack), fixed, office=office),
        encoding="utf-8",
    )
    path.chmod(
        path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    )


def add_windows_command(
    pack: Path,
    launcher: Path,
    name: str,
    target: Path,
    fixed: list[str] | None = None,
) -> None:
    exe = pack / "bin" / f"{name}.exe"
    shutil.copy2(launcher, exe)
    lines = [f"{{PACK}}/{relative_to_pack(target, pack)}"]
    lines.extend(fixed or [])
    (pack / "bin" / f"{name}.target").write_text(
        "\n".join(lines) + "\n",
        encoding="utf-8",
    )


def inventory(root: Path) -> list[dict[str, object]]:
    records = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and path.name != "pack-manifest.json":
            records.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "sha256": digest(path),
                    "size": path.stat().st_size,
                }
            )
    return records


def write_license_declarations(pack: Path, manifest: dict) -> None:
    licenses = pack / "licenses"
    licenses.mkdir(parents=True, exist_ok=True)
    for engine in manifest["engines"]:
        text = (
            f"FileFlow bundled engine: {engine['id']}\n"
            f"Declared upstream license: {engine['license']}\n"
            "Upstream package license files remain inside the private runtime "
            "when supplied by the package.\n"
        )
        (licenses / f"{engine['id']}.txt").write_text(
            text,
            encoding="utf-8",
        )



def capture_provenance(
    pack: Path,
    prefix: Path,
    runtime: Path,
    target: str,
    vendored_host_libraries: list[dict[str, object]],
) -> Path:
    conda_packages = []
    conda_meta = prefix / "conda-meta"
    if conda_meta.is_dir():
        for meta_path in sorted(conda_meta.glob("*.json")):
            try:
                raw = json.loads(meta_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                continue
            conda_packages.append({
                key: raw.get(key)
                for key in ("name", "version", "build", "build_number", "channel", "url", "sha256", "md5")
                if raw.get(key) is not None
            })

    python_packages = []
    for metadata in sorted(runtime.rglob("*.dist-info/METADATA")):
        name = version = None
        try:
            for line in metadata.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.startswith("Name: ") and name is None:
                    name = line[6:].strip()
                elif line.startswith("Version: ") and version is None:
                    version = line[9:].strip()
                if name and version:
                    break
        except OSError:
            continue
        if name and version:
            python_packages.append({"name": name, "version": version})

    payload = {
        "schemaVersion": 1,
        "target": target,
        "factory": "native-conda-forge+tdf-libreoffice",
        "condaPackages": conda_packages,
        "pythonPackages": python_packages,
        "vendoredHostLibraries": vendored_host_libraries,
    }
    destination = pack / "provenance" / "runtime-packages.json"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return destination

def expected_count(manifest: dict) -> int:
    return sum(len(engine["executables"]) for engine in manifest["engines"])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--prefix", type=Path, default=None)
    args = parser.parse_args()

    env_prefix = os.environ.get("CONDA_PREFIX", "").strip()
    if args.prefix is None and not env_prefix:
        raise SystemExit(
            "CONDA_PREFIX is missing; the micromamba engine environment "
            "was not activated"
        )
    raw_prefix = args.prefix if args.prefix is not None else Path(env_prefix)
    prefix = raw_prefix.expanduser().resolve()
    if not prefix.is_dir():
        raise SystemExit(
            "Conda/micromamba runtime prefix is missing; pass --prefix or "
            "activate the engine environment"
        )

    manifest = json.loads(MANIFEST.read_text())
    pack_version = str(manifest.get("packVersion", "")).strip()
    if not pack_version:
        raise SystemExit("engine manifest packVersion is missing")

    pack = PACK_ROOT / args.target
    if pack.exists():
        shutil.rmtree(pack)
    (pack / "bin").mkdir(parents=True)
    (pack / "share").mkdir(parents=True)

    runtime = pack / "share" / "runtime"
    log(f"copying private engine runtime from {prefix}")
    copy_runtime(prefix, runtime)
    office_target = install_libreoffice(pack, args.target)

    # The Windows command wrappers are native PE binaries themselves. Build the
    # canonical launcher before dependency closure so its imports are certified
    # and any non-system CRT/provider DLL is vendored exactly like every engine.
    # Unix wrappers are shell scripts and therefore introduce no native closure.
    launcher = build_windows_launcher(pack) if os.name == "nt" else None

    vendored_host_libraries: list[dict[str, object]] = []
    if sys.platform.startswith("linux"):
        vendored_host_libraries = vendor_linux_external_dependencies(pack, args.target)
    elif sys.platform == "darwin":
        vendored_host_libraries = vendor_macos_external_dependencies(pack, args.target, prefix)
    elif os.name == "nt":
        vendored_host_libraries = vendor_windows_external_dependencies(pack, args.target, prefix)
    provenance = capture_provenance(pack, prefix, runtime, args.target, vendored_host_libraries)
    log(f"captured exact runtime provenance -> {provenance.relative_to(pack)}")

    for canonical, aliases in DIRECT.items():
        target = find_runtime_program(runtime, aliases)
        if os.name == "nt":
            assert launcher is not None
            add_windows_command(pack, launcher, canonical, target)
        else:
            add_unix_command(pack, canonical, target)

    exif_target, exif_fixed = find_exiftool(runtime, pack)
    if os.name == "nt":
        assert launcher is not None
        add_windows_command(
            pack,
            launcher,
            "exiftool",
            exif_target,
            exif_fixed,
        )
    else:
        add_unix_command(pack, "exiftool", exif_target, exif_fixed)

    python = find_python(runtime)
    for canonical, module in PYTHON_MODULES.items():
        if os.name == "nt":
            assert launcher is not None
            add_windows_command(
                pack,
                launcher,
                canonical,
                python,
                ["-m", module],
            )
        else:
            add_unix_command(
                pack,
                canonical,
                python,
                ["-m", module],
            )

    if os.name == "nt":
        assert launcher is not None
        add_windows_command(pack, launcher, "soffice", office_target)
    else:
        add_unix_command(pack, "soffice", office_target, office=True)

    if launcher is not None:
        launcher.unlink(missing_ok=True)

    write_license_declarations(pack, manifest)

    missing = []
    for engine in manifest["engines"]:
        for executable in engine["executables"]:
            variants = (
                [executable, f"{executable}.exe"]
                if os.name == "nt"
                else [executable]
            )
            if not any(
                (pack / "bin" / name).is_file() for name in variants
            ):
                missing.append(f"{engine['id']}:{executable}")
    if missing:
        raise SystemExit(
            "FULL engine pack incomplete: " + ", ".join(missing)
        )

    pack_manifest = {
        "schemaVersion": 2,
        "packVersion": pack_version,
        "target": args.target,
        "flavor": "full",
        "expectedExecutableCount": expected_count(manifest),
        "factory": "native-conda-forge+tdf-libreoffice",
        "provenanceSha256": digest(provenance),
        "files": inventory(pack),
    }
    (pack / "pack-manifest.json").write_text(
        json.dumps(pack_manifest, indent=2) + "\n",
        encoding="utf-8",
    )

    log(
        f"FULL pack ready: target={args.target} "
        f"commands={expected_count(manifest)} "
        f"files={len(pack_manifest['files'])}"
    )


if __name__ == "__main__":
    main()
