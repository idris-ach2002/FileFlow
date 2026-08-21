#!/usr/bin/env python3
"""Stage a FileFlow-owned native runtime for the current CI host.

The runtime is produced on the same OS/architecture as the final Tauri package.
Native engines are isolated under runtime/tools/<engine>/ so one engine's shared
libraries cannot leak into another engine. Python-backed engines use a separate
runtime/python prefix. LibreOffice and ExifTool deliberately remain host
fallbacks because their native installations are not reliably relocatable.
"""
from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = ROOT / "src-tauri" / "runtime"

ENGINE_SPECS: dict[str, list[str]] = {
    "ffmpeg": ["ffmpeg"],
    "vips": ["vips"],
    "imagemagick": ["magick", "convert"],
    "qpdf": ["qpdf"],
    "poppler": ["pdftoppm", "pdftotext"],
    "ghostscript": ["gswin64c", "gswin32c", "gs"],
    "tesseract": ["tesseract"],
    "pandoc": ["pandoc"],
    "archive": ["7zz", "7z"],
    "zstd": ["zstd"],
    "lz4": ["lz4"],
}
PYTHON_ENGINES = {"img2pdf": "img2pdf", "ocr": "ocrmypdf"}
CORE_ENGINES = {
    "ffmpeg", "vips", "imagemagick", "qpdf", "img2pdf", "poppler",
    "ghostscript", "tesseract", "ocr", "archive", "zstd", "lz4",
}
FALLBACK_ONLY_ENGINES = {"office", "metadata"}
LINUX_SYSTEM_LIB_PREFIXES = (
    "libc.so", "libm.so", "libpthread.so", "libdl.so", "librt.so",
    "libresolv.so", "libutil.so", "libcrypt.so", "libnsl.so",
    "ld-linux", "ld64.so",
)
WINDOWS_SHIM_MARKERS = (
    "/chocolatey/bin/", "/scoop/shims/", "/microsoft/winget/links/",
    "/microsoft/windowsapps/",
)


def run(*args: str, check: bool = True, capture: bool = False, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(args), check=check, text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture else None,
        env=env,
    )


def target_family(target: str) -> str:
    low = target.lower()
    if "windows" in low:
        return "windows"
    if "apple" in low or "darwin" in low:
        return "macos"
    if "linux" in low:
        return "linux"
    raise SystemExit(f"unsupported target: {target}")


def executable_names(names: list[str], family: str) -> list[str]:
    if family != "windows":
        return names
    out: list[str] = []
    for name in names:
        out.append(name if name.lower().endswith(".exe") else f"{name}.exe")
        out.append(name)
    return list(dict.fromkeys(out))


def is_windows_shim(path: Path) -> bool:
    normalized = f"/{str(path).replace(chr(92), '/').lower().strip('/')}/"
    return any(marker in normalized for marker in WINDOWS_SHIM_MARKERS)


def search_roots(family: str) -> list[Path]:
    roots = [Path(p) for p in os.environ.get("PATH", "").split(os.pathsep) if p]
    home = Path.home()
    if family == "linux":
        roots += [Path("/usr/bin"), Path("/usr/local/bin"), home / ".local/bin", Path("/home/linuxbrew/.linuxbrew/bin")]
    elif family == "macos":
        roots += [Path("/opt/homebrew/bin"), Path("/usr/local/bin"), Path("/usr/bin"), home / ".local/bin"]
    else:
        local = Path(os.environ.get("LOCALAPPDATA", home))
        profile = Path(os.environ.get("USERPROFILE", home))
        chocolatey = Path(os.environ.get("ChocolateyInstall", "C:/ProgramData/chocolatey"))
        for value in (
            os.environ.get("ProgramFiles"), os.environ.get("ProgramFiles(x86)"),
            str(local / "Programs"), str(local / "Microsoft/WinGet/Packages"),
            str(chocolatey / "lib"), str(profile / "scoop/apps"),
        ):
            if value:
                roots.append(Path(value))
    return list(dict.fromkeys(roots))


def find_program(names: list[str], family: str) -> Path | None:
    wanted = {name.lower() for name in executable_names(names, family)}

    # On Windows, package managers frequently put launch shims in PATH. Those
    # shims reference the CI host installation and are not distributable, so
    # prefer the real package directory first.
    if family == "windows":
        for root in search_roots(family):
            if not root.is_dir() or is_windows_shim(root):
                continue
            for current, dirs, files in os.walk(root):
                current_path = Path(current)
                try:
                    depth = len(current_path.relative_to(root).parts)
                except ValueError:
                    depth = 99
                if depth >= 6:
                    dirs[:] = []
                lower = {f.lower(): f for f in files}
                for name in wanted:
                    if name in lower:
                        candidate = (current_path / lower[name]).resolve()
                        if not is_windows_shim(candidate):
                            return candidate

    for name in executable_names(names, family):
        found = shutil.which(name)
        if found:
            candidate = Path(found).resolve()
            if family != "windows" or not is_windows_shim(candidate):
                return candidate

    for root in search_roots(family):
        if not root.is_dir():
            continue
        for name in wanted:
            candidate = root / name
            if candidate.is_file():
                return candidate.resolve()
    return None


def chmod_exec(path: Path) -> None:
    if os.name != "nt":
        path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def linux_ldd(path: Path) -> list[tuple[str, Path]]:
    proc = run("ldd", str(path), check=False, capture=True)
    if proc.returncode != 0 or not proc.stdout:
        return []
    result: list[tuple[str, Path]] = []
    for raw in proc.stdout.splitlines():
        line = raw.strip()
        match = re.match(r"([^\s]+)\s+=>\s+(/[^\s]+)", line)
        if match:
            result.append((match.group(1), Path(match.group(2))))
            continue
        match = re.match(r"(/[^\s]+)", line)
        if match:
            p = Path(match.group(1))
            result.append((p.name, p))
    return result


def collect_linux_dependencies(entry: Path, libdir: Path) -> None:
    libdir.mkdir(parents=True, exist_ok=True)
    queue = [entry]
    seen: set[Path] = set()
    while queue:
        current = queue.pop()
        try:
            real = current.resolve()
        except OSError:
            continue
        if real in seen:
            continue
        seen.add(real)
        for soname, source in linux_ldd(real):
            if soname.startswith(LINUX_SYSTEM_LIB_PREFIXES):
                continue
            try:
                source_real = source.resolve(strict=True)
            except OSError:
                continue
            destination = libdir / soname
            if not destination.exists():
                shutil.copy2(source_real, destination)
                queue.append(destination)


def set_linux_rpath(path: Path, rpath: str) -> None:
    # Shell wrappers and static ELF executables do not need an RPATH. In
    # particular Debian/Ubuntu expose 7z through a wrapper under /usr/bin.
    try:
        if path.read_bytes()[:4] != b"\x7fELF":
            return
    except OSError:
        return
    if not linux_ldd(path):
        return
    patchelf = shutil.which("patchelf")
    if not patchelf:
        raise RuntimeError("patchelf is required to build the relocatable Linux runtime")
    proc = run(patchelf, "--set-rpath", rpath, str(path), check=False, capture=True)
    if proc.returncode != 0:
        detail = (proc.stdout or "").strip()
        raise RuntimeError(f"patchelf failed for {path}: {detail or f'exit {proc.returncode}'}")


def patch_linux_libdir(libdir: Path) -> None:
    if not libdir.is_dir():
        return
    for path in libdir.iterdir():
        if path.is_file() and (".so" in path.name or path.suffix == ".so"):
            set_linux_rpath(path, "$ORIGIN")


def stage_linux_closure(binary: Path, tool_root: Path) -> None:
    libdir = tool_root / "lib"
    collect_linux_dependencies(binary, libdir)
    set_linux_rpath(binary, "$ORIGIN/../lib")
    patch_linux_libdir(libdir)


def patch_linux_module_closure(module: Path, tool_root: Path) -> None:
    libdir = tool_root / "lib"
    collect_linux_dependencies(module, libdir)
    relative = os.path.relpath(libdir, module.parent).replace(os.sep, "/")
    set_linux_rpath(module, f"$ORIGIN/{relative}" if relative != "." else "$ORIGIN")
    patch_linux_libdir(libdir)


def mac_dependencies(path: Path) -> list[Path]:
    proc = run("otool", "-L", str(path), check=False, capture=True)
    if proc.returncode != 0 or not proc.stdout:
        return []
    deps: list[Path] = []
    for line in proc.stdout.splitlines()[1:]:
        value = line.strip().split(" (", 1)[0]
        if value.startswith("/") and not value.startswith(("/System/Library/", "/usr/lib/")):
            deps.append(Path(value))
    return deps


def stage_macos_closure(binary: Path, tool_root: Path) -> None:
    libdir = tool_root / "lib"
    libdir.mkdir(parents=True, exist_ok=True)
    queue = [binary]
    copied: dict[Path, Path] = {}
    while queue:
        current = queue.pop()
        for dep in mac_dependencies(current):
            try:
                real = dep.resolve(strict=True)
            except OSError:
                continue
            if real in copied:
                continue
            dest = libdir / real.name
            if not dest.exists():
                shutil.copy2(real, dest)
                chmod_exec(dest)
                queue.append(dest)
            copied[real] = dest

    source_to_name = {source: dest.name for source, dest in copied.items()}
    all_files = [binary, *dict.fromkeys(copied.values())]
    for current in all_files:
        for dep in mac_dependencies(current):
            try:
                real = dep.resolve(strict=True)
            except OSError:
                continue
            name = source_to_name.get(real)
            if not name:
                # The same dylib may already have been staged by an earlier
                # closure pass for this engine.
                existing = libdir / real.name
                if existing.is_file():
                    name = existing.name
                else:
                    continue
            relative = os.path.relpath(libdir, current.parent).replace(os.sep, "/")
            replacement = f"@loader_path/{name}" if relative == "." else f"@loader_path/{relative}/{name}"
            run("install_name_tool", "-change", str(dep), replacement, str(current), check=False)
        if current.parent == libdir:
            run("install_name_tool", "-id", f"@loader_path/{current.name}", str(current), check=False)


def copy_tree_dereferenced(source: Path, dest: Path, seen: set[Path] | None = None) -> None:
    """Copy a resource tree while materializing symlink targets.

    Distribution packages (notably Ghostscript on Debian/Ubuntu) use symlinks
    from their resource tree into /usr/share/fonts, /usr/share/color and
    /var/lib. Preserving those links would make the bundled runtime depend on
    the target machine, while shutil.copytree(..., symlinks=False) can treat
    valid relative links as dangling. Materialize the target instead.
    """
    if seen is None:
        seen = set()
    try:
        resolved = source.resolve(strict=True)
    except OSError:
        return
    if resolved.is_dir():
        if resolved in seen:
            return
        seen.add(resolved)
        dest.mkdir(parents=True, exist_ok=True)
        try:
            children = list(resolved.iterdir())
        except OSError:
            return
        for child in children:
            copy_tree_dereferenced(child, dest / child.name, seen)
        seen.remove(resolved)
        return
    if resolved.is_file():
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(resolved, dest)


def copy_tree_if_present(source: Path, dest: Path) -> None:
    if source.exists() and not dest.exists():
        copy_tree_dereferenced(source, dest)


def copy_tessdata(source_root: Path, dest: Path) -> None:
    if not source_root.is_dir():
        return
    dest.mkdir(parents=True, exist_ok=True)
    # FileFlow invokes Tesseract with fra+eng and OCRmyPDF may use OSD. Shipping
    # every language pack can add hundreds of megabytes for no current benefit.
    for name in ("fra.traineddata", "eng.traineddata", "osd.traineddata", "pdf.ttf"):
        candidate = source_root / name
        if candidate.is_file():
            shutil.copy2(candidate, dest / name)
    for dirname in ("configs", "tessconfigs"):
        copy_tree_if_present(source_root / dirname, dest / dirname)


def copy_linux_engine_data(engine_id: str, tool_root: Path) -> None:
    if engine_id == "poppler":
        copy_tree_if_present(Path("/usr/share/poppler"), tool_root / "share/poppler")
    elif engine_id == "ghostscript":
        copy_tree_if_present(Path("/usr/share/ghostscript"), tool_root / "share/ghostscript")
    elif engine_id == "tesseract":
        tess_roots = sorted(Path("/usr/share/tesseract-ocr").glob("*/tessdata"))
        if tess_roots:
            copy_tessdata(tess_roots[-1], tool_root / "share/tessdata")
    elif engine_id == "imagemagick":
        for source in Path("/etc").glob("ImageMagick-*"):
            copy_tree_if_present(source, tool_root / "etc" / source.name)
        for source in Path("/usr/lib").glob("*/ImageMagick-*"):
            copy_tree_if_present(source, tool_root / "lib" / source.name)
    elif engine_id == "vips":
        for source in Path("/usr/lib").glob("*/vips-modules-*"):
            copy_tree_if_present(source, tool_root / "lib" / source.name)

    # ImageMagick and libvips load codecs with dlopen(). Those modules are not
    # part of the executable's normal ldd graph, so make each module relocatable
    # against this engine's private lib directory as well.
    libdir = tool_root / "lib"
    if engine_id in {"imagemagick", "vips"} and libdir.is_dir():
        modules = [p for p in libdir.rglob("*") if p.is_file() and ".so" in p.name]
        for module in modules:
            patch_linux_module_closure(module, tool_root)


def mac_formula_prefix(source: Path) -> Path:
    if source.parent.name == "bin":
        return source.parent.parent
    return source.parent


def copy_macos_engine_data(engine_id: str, source: Path, tool_root: Path) -> None:
    prefix = mac_formula_prefix(source)
    if engine_id == "poppler":
        copy_tree_if_present(prefix / "share/poppler", tool_root / "share/poppler")
    elif engine_id == "ghostscript":
        copy_tree_if_present(prefix / "share/ghostscript", tool_root / "share/ghostscript")
    elif engine_id == "tesseract":
        copy_tessdata(prefix / "share/tessdata", tool_root / "share/tessdata")
    elif engine_id == "imagemagick":
        for source_dir in (prefix / "etc").glob("ImageMagick-*") if (prefix / "etc").is_dir() else []:
            copy_tree_if_present(source_dir, tool_root / "etc" / source_dir.name)
        for source_dir in (prefix / "lib").glob("ImageMagick-*") if (prefix / "lib").is_dir() else []:
            copy_tree_if_present(source_dir, tool_root / "lib" / source_dir.name)
    elif engine_id == "vips":
        for source_dir in (prefix / "lib").glob("vips-modules-*") if (prefix / "lib").is_dir() else []:
            copy_tree_if_present(source_dir, tool_root / "lib" / source_dir.name)

    if engine_id in {"imagemagick", "vips"}:
        libdir = tool_root / "lib"
        if libdir.is_dir():
            for module in [p for p in libdir.rglob("*") if p.is_file() and (p.suffix in {".dylib", ".so"} or ".so" in p.name)]:
                stage_macos_closure(module, tool_root)



def resolve_linux_archive_source(source: Path) -> tuple[Path, Path | None]:
    """Resolve Debian/Ubuntu's /usr/bin/7z wrapper to its real payload."""
    if source.name not in {"7z", "7za", "7zr"}:
        return source, None
    for package_root in (Path("/usr/lib/7zip"), Path("/usr/lib/p7zip")):
        candidate = package_root / source.name
        if candidate.is_file():
            return candidate, package_root
    return source, None

def windows_engine_root(engine_id: str, source: Path) -> Path:
    parent = source.parent
    # Packages such as Ghostscript/Poppler/libvips/qpdf often keep DLLs in bin
    # and data/resources in sibling directories. Copy the package prefix rather
    # than only bin in those cases.
    if parent.name.lower() in {"bin", "program"} and engine_id in {
        "vips", "imagemagick", "qpdf", "poppler", "ghostscript", "tesseract",
    }:
        return parent.parent
    return parent


def stage_native_engine(engine_id: str, source: Path, runtime: Path, family: str) -> str:
    tools = runtime / "tools" / engine_id
    if tools.exists():
        shutil.rmtree(tools)

    if family == "windows":
        root = windows_engine_root(engine_id, source)
        shutil.copytree(root, tools, symlinks=False, ignore_dangling_symlinks=True)
        relative_inside = source.relative_to(root)
        return (Path("tools") / engine_id / relative_inside).as_posix()

    archive_root: Path | None = None
    if family == "linux" and engine_id == "archive":
        source, archive_root = resolve_linux_archive_source(source)

    bindir = tools / "bin"
    bindir.mkdir(parents=True, exist_ok=True)
    dest = bindir / source.name
    shutil.copy2(source, dest)
    chmod_exec(dest)
    if family == "linux":
        stage_linux_closure(dest, tools)

        if archive_root is not None:
            for name in ("7z.so", "7zCon.sfx"):
                companion = archive_root / name
                if companion.is_file():
                    copied = bindir / name
                    shutil.copy2(companion, copied)
                    if name.endswith(".so"):
                        patch_linux_module_closure(copied, tools)
            codecs = archive_root / "Codecs"
            if codecs.is_dir():
                shutil.copytree(codecs, bindir / "Codecs", symlinks=False, ignore_dangling_symlinks=True)
                for module in (bindir / "Codecs").rglob("*.so"):
                    patch_linux_module_closure(module, tools)

        copy_linux_engine_data(engine_id, tools)
    else:
        stage_macos_closure(dest, tools)
        copy_macos_engine_data(engine_id, source, tools)
    return dest.relative_to(runtime).as_posix()


def safe_python_prefix(prefix: Path) -> bool:
    resolved = prefix.resolve()
    if os.name == "nt":
        return True
    return resolved not in {Path("/usr"), Path("/usr/local"), Path("/")}


def python_executable(prefix: Path, family: str) -> Path:
    if family == "windows":
        return prefix / "python.exe"
    for candidate in (prefix / "bin/python3", prefix / "bin/python"):
        if candidate.is_file():
            return candidate
    return prefix / "bin/python3"


def compile_windows_python_launcher(runtime: Path, engine_id: str, module: str, target: str) -> Path:
    source = runtime / f".{engine_id}-launcher.rs"
    output = runtime / "bin" / f"{engine_id}.exe"
    source.write_text(
        f'''use std::{{env, path::PathBuf, process::Command}};\nfn main() {{\n    let exe = env::current_exe().expect("current exe");\n    let root = exe.parent().and_then(|p| p.parent()).expect("runtime root");\n    let python: PathBuf = root.join("python").join("python.exe");\n    let mut cmd = Command::new(python);\n    cmd.arg("-m").arg("{module}").args(env::args_os().skip(1));\n    let status = cmd.status().expect("launch FileFlow Python runtime");\n    std::process::exit(status.code().unwrap_or(1));\n}}\n''',
        encoding="utf-8",
    )
    run("rustc", "--edition", "2024", "--target", target, "-O", str(source), "-o", str(output))
    source.unlink(missing_ok=True)
    return output


def stage_python(runtime: Path, family: str, target: str) -> dict[str, str]:
    source_prefix = Path(sys.base_prefix)
    if not safe_python_prefix(source_prefix):
        print(f"[WARN] refusing to copy non-isolated Python prefix: {source_prefix}")
        return {}

    destination = runtime / "python"
    if destination.exists():
        shutil.rmtree(destination)
    print(f"[runtime] copying Python prefix: {source_prefix}")
    shutil.copytree(
        source_prefix, destination, symlinks=False,
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "test", "tests", "idle_test"),
    )
    py = python_executable(destination, family)
    if not py.is_file():
        print(f"[WARN] staged Python executable missing: {py}")
        return {}
    chmod_exec(py)

    if family == "linux":
        stage_linux_closure(py, destination)
    elif family == "macos":
        stage_macos_closure(py, destination)

    env = os.environ.copy()
    for key in ("PYTHONHOME", "PYTHONPATH", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH"):
        env.pop(key, None)
    env["PYTHONHOME"] = str(destination)

    proc = subprocess.run(
        [str(py), "-m", "pip", "install", "--disable-pip-version-check", "--no-input", "--upgrade", "img2pdf", "ocrmypdf"],
        env=env,
    )
    if proc.returncode != 0:
        print("[WARN] unable to install Python engines into packaged runtime")
        return {}

    bindir = runtime / "bin"
    bindir.mkdir(parents=True, exist_ok=True)
    result: dict[str, str] = {}
    for engine_id, module in PYTHON_ENGINES.items():
        if family == "windows":
            launcher = compile_windows_python_launcher(runtime, engine_id, module, target)
        else:
            launcher = bindir / engine_id
            launcher.write_text(
                "#!/usr/bin/env sh\n"
                "set -eu\n"
                'ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)\n'
                f'exec "$ROOT/python/bin/python3" -m {module} "$@"\n',
                encoding="utf-8",
            )
            chmod_exec(launcher)
        result[engine_id] = launcher.relative_to(runtime).as_posix()
    return result


def write_manifest(runtime: Path, target: str, engines: dict[str, str]) -> None:
    manifest = {
        "version": 1,
        "target": target,
        "python": platform.python_version(),
        "engines": dict(sorted(engines.items())),
    }
    (runtime / "runtime-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--strict", action="store_true", help="fail if a core runtime engine cannot be staged")
    args = parser.parse_args()
    family = target_family(args.target)
    runtime = args.output.resolve()
    if runtime.exists():
        shutil.rmtree(runtime)
    runtime.mkdir(parents=True)

    engines: dict[str, str] = {}
    print(f"[runtime] host fallback only: {sorted(FALLBACK_ONLY_ENGINES)}")
    for engine_id, names in ENGINE_SPECS.items():
        source = find_program(names, family)
        if not source:
            print(f"[MISS] {engine_id}: {', '.join(names)}")
            continue
        try:
            relative = stage_native_engine(engine_id, source, runtime, family)
        except Exception as exc:
            print(f"[WARN] {engine_id}: staging failed: {exc}")
            continue
        engines[engine_id] = relative
        print(f"[OK]   {engine_id}: {source} -> {relative}")

    try:
        engines.update(stage_python(runtime, family, args.target))
    except Exception as exc:
        print(f"[WARN] Python runtime staging failed: {exc}")

    write_manifest(runtime, args.target, engines)
    missing_core = sorted(CORE_ENGINES.difference(engines))
    print(f"[runtime] staged {len(engines)} engines; missing core={missing_core or 'none'}")
    if args.strict and missing_core:
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
