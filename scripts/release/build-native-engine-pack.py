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

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "release/engines/manifest.json"
PACK_ROOT = ROOT / "release/engines/packs"

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


def install_libreoffice(pack: Path) -> Path:
    destination = pack / "share" / "libreoffice"
    if destination.exists():
        shutil.rmtree(destination)

    if sys.platform.startswith("linux"):
        source = Path("/usr/lib/libreoffice")
        if not source.is_dir():
            raise SystemExit(
                "LibreOffice is missing. CI must install libreoffice-core/libreoffice-writer."
            )
        copytree(source, destination)
        target = destination / "program" / "soffice"
    elif sys.platform == "darwin":
        source = Path("/Applications/LibreOffice.app/Contents")
        if not source.is_dir():
            raise SystemExit(
                "LibreOffice.app is missing. CI must install the libreoffice cask."
            )
        copytree(source, destination / "Contents")
        target = destination / "Contents" / "MacOS" / "soffice"
    elif os.name == "nt":
        roots = [
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "LibreOffice",
            Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "LibreOffice",
        ]
        source = next(
            (root for root in roots if (root / "program" / "soffice.exe").is_file()),
            None,
        )
        if source is None:
            raise SystemExit(
                "LibreOffice is missing. CI must install chocolatey libreoffice-fresh."
            )
        copytree(source, destination)
        target = destination / "program" / "soffice.exe"
    else:
        raise SystemExit(f"unsupported host for LibreOffice: {sys.platform}")

    if not target.is_file():
        raise SystemExit(f"LibreOffice launcher missing after copy: {target}")
    return target


def shell_arg(arg: str) -> str:
    if arg.startswith("{PACK}/"):
        relative = arg[len("{PACK}/") :].replace('"', '\\"')
        return f'"$PACK_ROOT/{relative}"'
    return shlex.quote(arg)


def unix_wrapper(target_rel: str, fixed_args: list[str] | None = None) -> str:
    fixed_args = fixed_args or []
    fixed = " ".join(shell_arg(arg) for arg in fixed_args)
    target = target_rel.replace('"', '\\"')
    spacer = f" {fixed}" if fixed else ""
    return f'''#!/bin/sh
set -eu
BIN_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
PACK_ROOT="$(CDPATH= cd -- "$BIN_DIR/.." && pwd)"
RUNTIME="$PACK_ROOT/share/runtime"

# Zero-dependency host contract: never inherit Conda/Python/library paths from
# the machine that happens to launch FileFlow. Engine-to-engine subprocesses
# resolve through FileFlow's own wrappers first.
unset CONDA_PREFIX CONDA_DEFAULT_ENV CONDA_EXE MAMBA_EXE PYTHONPATH LD_PRELOAD
export PYTHONNOUSERSITE=1
export PATH="$BIN_DIR:$RUNTIME/bin:$RUNTIME/Library/bin:$RUNTIME/Scripts:/usr/bin:/bin"
[ ! -d "$RUNTIME/lib" ] || export LD_LIBRARY_PATH="$RUNTIME/lib"
[ ! -d "$RUNTIME/lib" ] || export DYLD_LIBRARY_PATH="$RUNTIME/lib"
export PYTHONHOME="$RUNTIME"

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

fn configured_private_paths(pack: &Path, bin: &Path) -> Vec<PathBuf> {
    let mut values = vec![bin.to_path_buf()];
    let config = pack.join("engine-runtime-paths.txt");
    if let Ok(text) = fs::read_to_string(config) {
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let path = pack.join(line.replace('/', "\\"));
            if path.is_dir() && !values.contains(&path) {
                values.push(path);
            }
        }
    }
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
    let private_paths = configured_private_paths(pack, bin);
    set_private_path(&private_paths);
    configure_engine_environment(&runtime);

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
) -> None:
    path = pack / "bin" / name
    path.write_text(
        unix_wrapper(relative_to_pack(target, pack), fixed),
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



def capture_provenance(pack: Path, prefix: Path, runtime: Path, target: str) -> Path:
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
        "factory": "native-conda-forge+libreoffice",
        "condaPackages": conda_packages,
        "pythonPackages": python_packages,
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
    office_target = install_libreoffice(pack)
    provenance = capture_provenance(pack, prefix, runtime, args.target)
    log(f"captured exact runtime provenance -> {provenance.relative_to(pack)}")

    launcher = build_windows_launcher(pack) if os.name == "nt" else None

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
        add_unix_command(pack, "soffice", office_target)

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
        "factory": "native-conda-forge+libreoffice",
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
