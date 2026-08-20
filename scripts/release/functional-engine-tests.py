#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import json
import os
import subprocess
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_RESOURCE_ROOT = ROOT / "src-tauri/resources/engines"
DEFAULT_META = ROOT / "src-tauri/resources/engine-pack.json"
PNG_1X1 = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
)


def clean_base_environment() -> dict[str, str]:
    keep = (
        "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR", "TMP", "TEMP",
        "SystemRoot", "WINDIR", "USERPROFILE", "APPDATA", "LOCALAPPDATA",
    )
    env = {key: value for key in keep if (value := os.environ.get(key))}
    if os.name == "nt":
        root = env.get("SystemRoot", r"C:\Windows")
        env["PATH"] = os.pathsep.join([str(Path(root) / "System32"), root])
    else:
        env["PATH"] = "/usr/bin:/bin"
    env["PYTHONNOUSERSITE"] = "1"
    return env


def env_for_pack(resource_root: Path) -> dict[str, str]:
    env = clean_base_environment()
    bin_dir = resource_root / "bin"
    runtime = resource_root / "share" / "runtime"
    entries = [bin_dir]
    if os.name == "nt":
        path_file = resource_root / "engine-runtime-paths.txt"
        if path_file.is_file():
            for line in path_file.read_text(encoding="utf-8").splitlines():
                line = line.strip()
                if line:
                    candidate = resource_root / line
                    if candidate.is_dir():
                        entries.append(candidate)
    else:
        for candidate in (runtime / "bin", runtime / "Library" / "bin", runtime / "Scripts"):
            if candidate.is_dir():
                entries.append(candidate)
    entries.append(Path(env["PATH"].split(os.pathsep)[0]))
    env["PATH"] = os.pathsep.join(str(entry) for entry in entries)

    if runtime.is_dir():
        env["PYTHONHOME"] = str(runtime)
    if os.name != "nt" and (runtime / "lib").is_dir():
        key = "DYLD_LIBRARY_PATH" if sys_platform() == "darwin" else "LD_LIBRARY_PATH"
        env[key] = str(runtime / "lib")

    for tess in (runtime / "share" / "tessdata", runtime / "Library" / "share" / "tessdata"):
        if tess.is_dir():
            env["TESSDATA_PREFIX"] = str(tess)
            break
    env["MAGICK_HOME"] = str(runtime)
    for parent in (runtime / "etc", runtime / "share", runtime / "Library" / "etc", runtime / "Library" / "share"):
        if parent.is_dir():
            magick = next((p for p in sorted(parent.glob("ImageMagick-*")) if p.is_dir()), None)
            if magick:
                env["MAGICK_CONFIGURE_PATH"] = str(magick)
                break
    for lib_root in (runtime / "lib", runtime / "Library" / "lib"):
        if lib_root.is_dir():
            coders = next((p for p in sorted(lib_root.glob("ImageMagick-*/modules-*/coders")) if p.is_dir()), None)
            if coders:
                env["MAGICK_CODER_MODULE_PATH"] = str(coders)
                break
    gs_paths = []
    for gs_root in (runtime / "share" / "ghostscript", runtime / "Library" / "share" / "ghostscript"):
        if gs_root.is_dir():
            versions = sorted(p for p in gs_root.iterdir() if p.is_dir())
            if versions:
                gs_paths.extend(p for p in (versions[-1] / "Resource" / "Init", versions[-1] / "lib") if p.is_dir())
    if gs_paths:
        env["GS_LIB"] = os.pathsep.join(map(str, gs_paths))
    return env


def sys_platform() -> str:
    import sys
    return sys.platform


def minimal_pdf(path: Path) -> None:
    objects = [
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] /Contents 4 0 R >>\nendobj\n",
        b"4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n",
    ]
    data = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for obj in objects:
        offsets.append(len(data))
        data.extend(obj)
    xref = len(data)
    data.extend(f"xref\n0 {len(objects)+1}\n".encode())
    data.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        data.extend(f"{offset:010d} 00000 n \n".encode())
    data.extend(f"trailer\n<< /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    path.write_bytes(data)


def write_zip(path: Path, files: dict[str, str]) -> None:
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for name, text in files.items():
            bundle.writestr(name, text)


def minimal_docx(path: Path) -> None:
    write_zip(path, {
        "[Content_Types].xml": '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
        "_rels/.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
        "word/document.xml": '<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>FileFlow DOCX fixture</w:t></w:r></w:p><w:sectPr/></w:body></w:document>',
    })


def minimal_xlsx(path: Path) -> None:
    write_zip(path, {
        "[Content_Types].xml": '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>',
        "_rels/.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
        "xl/workbook.xml": '<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>',
        "xl/_rels/workbook.xml.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>',
        "xl/worksheets/sheet1.xml": '<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>FileFlow XLSX fixture</t></is></c></row></sheetData></worksheet>',
    })


def minimal_pptx(path: Path) -> None:
    write_zip(path, {
        "[Content_Types].xml": '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>',
        "_rels/.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>',
        "ppt/presentation.xml": '<?xml version="1.0"?><p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>',
        "ppt/_rels/presentation.xml.rels": '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>',
        "ppt/slides/slide1.xml": '<?xml version="1.0"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld></p:sld>',
    })


def assert_file(path: Path, *, min_size: int = 1) -> None:
    if not path.is_file() or path.stat().st_size < min_size:
        raise RuntimeError(f"expected output missing/empty: {path}")


def run(exe: Path, args: list[str], env: dict[str, str], timeout: int = 60) -> str:
    result = subprocess.run(
        [str(exe), *args],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=timeout,
    )
    if result.returncode != 0:
        raise RuntimeError(f"{exe.name} exited {result.returncode}: {(result.stdout or '')[-1600:]}")
    return result.stdout or ""


def bundled_python(resource_root: Path) -> Path | None:
    runtime = resource_root / "share" / "runtime"
    candidates = [runtime / "python.exe"] if os.name == "nt" else [runtime / "bin" / "python", runtime / "bin" / "python3"]
    return next((path for path in candidates if path.is_file()), None)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["optional", "core", "full"], default="optional")
    parser.add_argument("--engine-root", type=Path, default=DEFAULT_RESOURCE_ROOT)
    parser.add_argument("--metadata", type=Path, default=DEFAULT_META)
    args = parser.parse_args()

    resource_root = args.engine_root.resolve()
    bin_dir = resource_root / "bin"
    meta = json.loads(args.metadata.read_text())
    available = {item["name"]: bin_dir / item["name"] for item in meta.get("engines", [])}
    env = env_for_pack(resource_root)
    failures: list[str] = []
    tested: list[str] = []

    with tempfile.TemporaryDirectory(prefix="fileflow-engine-functional-") as tmp:
        root = Path(tmp)
        png = root / "input.png"; png.write_bytes(PNG_1X1)
        ppm = root / "input.ppm"; ppm.write_text("P3\n2 2\n255\n255 0 0  0 255 0\n0 0 255  255 255 255\n")
        ocr_pgm = root / "ocr.pgm"; ocr_pgm.write_bytes(b"P5\n240 80\n255\n" + bytes([255]) * (240 * 80))
        pdf = root / "input.pdf"; minimal_pdf(pdf)
        text = root / "input.txt"; text.write_text("FileFlow engine functional test\n")
        md = root / "input.md"; md.write_text("# FileFlow\n\nfunctional engine test\n")
        docx = root / "fixture.docx"; minimal_docx(docx)
        xlsx = root / "fixture.xlsx"; minimal_xlsx(xlsx)
        pptx = root / "fixture.pptx"; minimal_pptx(pptx)

        def exe(name: str) -> Path | None:
            variants = [name, name + ".exe"]
            return next((available[v] for v in variants if v in available), None)

        tests: list[tuple[str, object]] = []
        ffmpeg = exe("ffmpeg"); ffprobe = exe("ffprobe")
        if ffmpeg:
            def ffmpeg_test():
                out = root / "ffmpeg.bmp"; run(ffmpeg, ["-y", "-loglevel", "error", "-i", str(png), str(out)], env); assert_file(out)
            tests.append(("ffmpeg", ffmpeg_test))
        if ffprobe:
            tests.append(("ffprobe", lambda: run(ffprobe, ["-v", "error", "-show_format", str(png)], env)))
        magick = exe("magick")
        if magick:
            def magick_test():
                out = root / "magick.bmp"; run(magick, [str(png), str(out)], env); assert_file(out)
            tests.append(("imagemagick", magick_test))
        vips = exe("vips")
        if vips:
            def vips_test():
                out = root / "vips.ppm"; run(vips, ["copy", str(ppm), str(out)], env); assert_file(out)
            tests.append(("vips", vips_test))
        qpdf = exe("qpdf")
        if qpdf:
            def qpdf_test():
                out = root / "qpdf.pdf"; run(qpdf, [str(pdf), str(out)], env); assert_file(out)
            tests.append(("qpdf", qpdf_test))
        seven = exe("7zz")
        if seven:
            def seven_test():
                archive = root / "fixture.7z"; out = root / "7z-out"; out.mkdir(); run(seven, ["a", "-bd", "-y", str(archive), str(text)], env); assert_file(archive); run(seven, ["x", "-bd", "-y", f"-o{out}", str(archive)], env); assert_file(out / text.name)
            tests.append(("archive", seven_test))
        zstd = exe("zstd")
        if zstd:
            def zstd_test():
                compressed = root / "input.zst"; output = root / "zstd.txt"; run(zstd, ["-q", "-f", str(text), "-o", str(compressed)], env); run(zstd, ["-q", "-d", "-f", str(compressed), "-o", str(output)], env); assert_file(output); assert output.read_bytes() == text.read_bytes()
            tests.append(("zstd", zstd_test))
        lz4 = exe("lz4")
        if lz4:
            def lz4_test():
                compressed = root / "input.lz4"; output = root / "lz4.txt"; run(lz4, ["-q", "-f", str(text), str(compressed)], env); run(lz4, ["-q", "-d", "-f", str(compressed), str(output)], env); assert_file(output); assert output.read_bytes() == text.read_bytes()
            tests.append(("lz4", lz4_test))
        exiftool = exe("exiftool")
        if exiftool:
            tests.append(("metadata", lambda: run(exiftool, ["-j", str(png)], env)))
        tesseract = exe("tesseract")
        if tesseract:
            tests.append(("tesseract", lambda: run(tesseract, [str(ocr_pgm), "stdout", "--psm", "7"], env)))
        pdftotext = exe("pdftotext"); pdftoppm = exe("pdftoppm")
        if pdftotext:
            def pdftotext_test():
                out = root / "poppler.txt"; run(pdftotext, [str(pdf), str(out)], env); assert_file(out, min_size=0)
            tests.append(("pdftotext", pdftotext_test))
        if pdftoppm:
            def pdftoppm_test():
                prefix = root / "poppler"; run(pdftoppm, ["-singlefile", "-f", "1", "-l", "1", str(pdf), str(prefix)], env); produced = list(root.glob("poppler.*"));
                if not produced: raise RuntimeError("pdftoppm produced no output")
            tests.append(("pdftoppm", pdftoppm_test))
        gs = exe("gs")
        if gs:
            def gs_test():
                out = root / "gs.pdf"; run(gs, ["-q", "-dBATCH", "-dNOPAUSE", "-sDEVICE=pdfwrite", f"-sOutputFile={out}", str(pdf)], env); assert_file(out)
            tests.append(("ghostscript", gs_test))
        pandoc = exe("pandoc")
        if pandoc:
            def pandoc_test():
                out = root / "pandoc.html"; run(pandoc, [str(md), "-o", str(out)], env); assert_file(out)
            tests.append(("pandoc", pandoc_test))
        soffice = exe("soffice")
        if soffice:
            def office_test():
                profile = root / "lo-profile"; profile.mkdir()
                for source in (docx, xlsx, pptx):
                    out_dir = root / f"lo-{source.suffix[1:]}"; out_dir.mkdir()
                    run(soffice, [f"-env:UserInstallation={profile.as_uri()}", "--headless", "--convert-to", "pdf", "--outdir", str(out_dir), str(source)], env, 180)
                    assert_file(out_dir / f"{source.stem}.pdf")
            tests.append(("office-docx-xlsx-pptx", office_test))
        ocr = exe("ocrmypdf")
        if ocr:
            def ocr_test():
                out = root / "ocr.pdf"; run(ocr, ["--skip-text", "--output-type", "pdf", str(pdf), str(out)], env, 180); assert_file(out)
            tests.append(("ocrmypdf", ocr_test))
        img2pdf = exe("img2pdf")
        if img2pdf:
            def img2pdf_test():
                out = root / "img2pdf.pdf"; run(img2pdf, [str(png), "-o", str(out)], env); assert_file(out)
            tests.append(("img2pdf", img2pdf_test))

        python = bundled_python(resource_root)
        if python:
            tests.append(("python-imports", lambda: run(python, ["-I", "-c", "import PIL, pikepdf, img2pdf, ocrmypdf; print('portable imports OK')"], env)))

        for name, test in tests:
            try:
                test()  # type: ignore[misc]
                tested.append(name)
                print(f"[OK] functional {name}")
            except Exception as error:
                failures.append(f"{name}: {error}")

    print(f"functional-tested {len(tested)} engine operation(s) in clean-host environment")
    if failures:
        print("functional engine failures:")
        for failure in failures:
            print("  -", failure)
        raise SystemExit(2)


if __name__ == "__main__":
    main()
