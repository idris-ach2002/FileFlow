#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

PROBES = {
    "ffmpeg": ["-version"],
    "vips": ["--version"],
    "imagemagick": ["-version"],
    "qpdf": ["--version"],
    "img2pdf": ["--version"],
    "poppler": ["-v"],
    "ghostscript": ["--version"],
    "tesseract": ["--version"],
    "ocr": ["--version"],
    "pandoc": ["--version"],
    "archive": ["i"],
    "zstd": ["--version"],
    "lz4": ["--version"],
}

CLEAN_KEYS = (
    "APPDIR", "APPIMAGE", "LD_LIBRARY_PATH", "LD_PRELOAD", "PYTHONHOME", "PYTHONPATH",
    "PERLLIB", "PERL5LIB", "GI_TYPELIB_PATH", "GIO_EXTRA_MODULES", "GSETTINGS_SCHEMA_DIR",
    "GTK_PATH", "QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "GST_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0", "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH", "MAGICK_HOME", "MAGICK_CONFIGURE_PATH",
    "MAGICK_CODER_MODULE_PATH", "MAGICK_FILTER_MODULE_PATH", "TESSDATA_PREFIX",
    "VIPS_PLUGIN_PATH", "GS_LIB",
)


def runtime_path_dirs(runtime: Path, manifest: dict) -> list[Path]:
    paths: list[Path] = [runtime / "bin"]
    for relative in manifest.get("engines", {}).values():
        parent = (runtime / relative).parent
        if parent not in paths:
            paths.append(parent)
    python_bin = runtime / ("python" if os.name == "nt" else "python/bin")
    if python_bin.is_dir() and python_bin not in paths:
        paths.append(python_bin)
    return paths


def tool_root_for(runtime: Path, executable: Path) -> Path | None:
    try:
        relative = executable.resolve().relative_to(runtime.resolve())
    except (OSError, ValueError):
        return None
    parts = relative.parts
    if len(parts) >= 3 and parts[0] == "tools":
        return runtime / "tools" / parts[1]
    return None


def ghostscript_lib_dirs(runtime: Path) -> list[Path]:
    base = runtime / "tools/ghostscript/share/ghostscript"
    if not base.is_dir():
        return []
    result: list[Path] = []
    for version in base.iterdir():
        if not version.is_dir():
            continue
        for rel in ("Resource/Init", "Resource/Font", "lib", "fonts"):
            candidate = version / rel
            if candidate.is_dir():
                result.append(candidate)
    return result


def sanitized_env(runtime: Path, manifest: dict, executable: Path) -> dict[str, str]:
    env = os.environ.copy()
    for key in CLEAN_KEYS:
        env.pop(key, None)

    path = [str(item) for item in runtime_path_dirs(runtime, manifest)]
    if env.get("PATH"):
        path.append(env["PATH"])
    env["PATH"] = os.pathsep.join(path)

    tool_root = tool_root_for(runtime, executable)
    if tool_root:
        libdir = tool_root / "lib"
        if sys.platform.startswith("linux") and libdir.is_dir():
            env["LD_LIBRARY_PATH"] = str(libdir)
        elif sys.platform == "darwin" and libdir.is_dir():
            env["DYLD_FALLBACK_LIBRARY_PATH"] = str(libdir)

    python = runtime / "python"
    if python.is_dir() and executable.parent == runtime / "bin":
        env["PYTHONHOME"] = str(python)

    tessdata = runtime / "tools/tesseract/share/tessdata"
    if tessdata.is_dir():
        env["TESSDATA_PREFIX"] = str(tessdata)

    gs_dirs = ghostscript_lib_dirs(runtime)
    if gs_dirs:
        env["GS_LIB"] = os.pathsep.join(str(path) for path in gs_dirs)

    magick_root = runtime / "tools/imagemagick"
    if magick_root.is_dir():
        env["MAGICK_HOME"] = str(magick_root)
        etc = magick_root / "etc"
        configs = [path for path in etc.glob("ImageMagick-*") if path.is_dir()] if etc.is_dir() else []
        if configs:
            env["MAGICK_CONFIGURE_PATH"] = os.pathsep.join(str(path) for path in configs)
        lib = magick_root / "lib"
        coders: list[Path] = []
        filters: list[Path] = []
        if lib.is_dir():
            for module_root in lib.glob("ImageMagick-*"):
                if not module_root.is_dir():
                    continue
                for modules in module_root.glob("modules-*"):
                    if (modules / "coders").is_dir():
                        coders.append(modules / "coders")
                    if (modules / "filters").is_dir():
                        filters.append(modules / "filters")
        if coders:
            env["MAGICK_CODER_MODULE_PATH"] = os.pathsep.join(str(path) for path in coders)
        if filters:
            env["MAGICK_FILTER_MODULE_PATH"] = os.pathsep.join(str(path) for path in filters)

    vips_root = runtime / "tools/vips/lib"
    if vips_root.is_dir():
        plugins = [path for path in vips_root.glob("vips-modules-*") if path.is_dir()]
        if plugins:
            env["VIPS_PLUGIN_PATH"] = os.pathsep.join(str(path) for path in plugins)

    return env



def run_checked(root: Path, manifest: dict, engine_id: str, args: list[str | Path], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    relative = manifest.get("engines", {}).get(engine_id)
    if not relative:
        raise RuntimeError(f"functional smoke requires engine: {engine_id}")
    executable = root / relative
    env = sanitized_env(root, manifest, executable)
    proc = subprocess.run(
        [str(executable), *[str(arg) for arg in args]],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, timeout=timeout,
    )
    if proc.returncode != 0:
        excerpt = "\n".join((proc.stdout or "").splitlines()[-12:])
        raise RuntimeError(f"{engine_id} functional probe failed ({proc.returncode}):\n{excerpt}")
    return proc


def functional_smoke(root: Path, manifest: dict) -> int:
    """Exercise real codecs/subprocess chains, not only --version probes."""
    failures = 0
    with tempfile.TemporaryDirectory(prefix="fileflow-runtime-smoke-") as tmp:
        work = Path(tmp)
        ppm = work / "source.ppm"
        # Portable pixmap generated with the Python stdlib only.
        ppm.write_bytes(b"P6\n4 4\n255\n" + bytes([255, 255, 255]) * 16)

        def check(label: str, fn) -> None:
            nonlocal failures
            try:
                fn()
                print(f"[OK]   functional {label}")
            except Exception as exc:
                failures += 1
                print(f"[FAIL] functional {label}: {exc}")

        png = work / "magick.png"
        check("imagemagick", lambda: run_checked(root, manifest, "imagemagick", [ppm, png]))

        vips_png = work / "vips.png"
        check("vips", lambda: run_checked(root, manifest, "vips", ["copy", ppm, vips_png]))

        tesseract_base = work / "ocr-image"
        check("tesseract", lambda: run_checked(root, manifest, "tesseract", [ppm, tesseract_base, "-l", "eng"]))

        ps = work / "input.ps"
        ps.write_text(
            "%!PS\n/Helvetica findfont 12 scalefont setfont\n72 720 moveto (FileFlow) show\nshowpage\n",
            encoding="ascii",
        )
        gs_pdf = work / "ghostscript.pdf"
        check(
            "ghostscript",
            lambda: run_checked(
                root, manifest, "ghostscript",
                ["-dBATCH", "-dNOPAUSE", "-sDEVICE=pdfwrite", f"-sOutputFile={gs_pdf}", ps],
            ),
        )

        check("qpdf", lambda: run_checked(root, manifest, "qpdf", ["--check", gs_pdf]))
        check("poppler", lambda: run_checked(root, manifest, "poppler", ["-png", "-singlefile", gs_pdf, work / "page"]))

        image_pdf = work / "image.pdf"
        check("img2pdf", lambda: run_checked(root, manifest, "img2pdf", [png, "-o", image_pdf]))

        ocr_pdf = work / "ocr.pdf"
        check(
            "ocr",
            lambda: run_checked(
                root, manifest, "ocr",
                ["--force-ocr", "--optimize", "0", "--output-type", "pdf", image_pdf, ocr_pdf],
                timeout=180,
            ),
        )

        frame = work / "frame.png"
        check(
            "ffmpeg",
            lambda: run_checked(
                root, manifest, "ffmpeg",
                ["-y", "-f", "lavfi", "-i", "color=c=white:s=16x16:d=0.1", "-frames:v", "1", frame],
            ),
        )

        markdown = work / "input.md"
        markdown.write_text("# FileFlow\n\nruntime smoke\n", encoding="utf-8")
        check("pandoc", lambda: run_checked(root, manifest, "pandoc", [markdown, "-o", work / "pandoc.html"]))

        payload = work / "payload.txt"
        payload.write_text("fileflow runtime\n", encoding="utf-8")
        check("archive", lambda: run_checked(root, manifest, "archive", ["a", "-y", work / "payload.7z", payload]))
        check("zstd", lambda: run_checked(root, manifest, "zstd", ["-f", payload, "-o", work / "payload.zst"]))
        check("lz4", lambda: run_checked(root, manifest, "lz4", ["-f", payload, work / "payload.lz4"]))

    return failures

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("src-tauri/runtime"))
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    manifest_path = root / "runtime-manifest.json"
    if not manifest_path.is_file():
        print(f"[FAIL] runtime manifest missing: {manifest_path}")
        return 2
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    failures = 0

    for engine_id, relative in manifest.get("engines", {}).items():
        relative_path = Path(relative)
        if relative_path.is_absolute() or ".." in relative_path.parts:
            failures += 1
            print(f"[FAIL] {engine_id}: unsafe manifest path {relative}")
            continue
        executable = root / relative_path
        if not executable.is_file():
            failures += 1
            print(f"[FAIL] {engine_id}: missing executable {executable}")
            continue
        probe = PROBES.get(engine_id, ["--version"])
        env = sanitized_env(root, manifest, executable)
        try:
            proc = subprocess.run(
                [str(executable), *probe], env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, timeout=45,
            )
            first = (proc.stdout or "").strip().splitlines()[:1]
            if proc.returncode == 0:
                print(f"[OK]   {engine_id}: {first[0] if first else executable}")
            else:
                failures += 1
                print(f"[FAIL] {engine_id}: exit={proc.returncode} {first[0] if first else ''}")
        except Exception as exc:
            failures += 1
            print(f"[FAIL] {engine_id}: {exc}")

    if args.strict and failures == 0:
        failures += functional_smoke(root, manifest)
    if args.strict and failures:
        return 2
    print(f"runtime smoke: {len(manifest.get('engines', {})) - failures} version probes OK, {failures} failures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
