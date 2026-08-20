#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
RESOURCE_ROOT=ROOT/'src-tauri/resources/engines'; BIN=RESOURCE_ROOT/'bin'; META=ROOT/'src-tauri/resources/engine-pack.json'
PNG_1X1=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=')


def env_for_pack() -> dict[str,str]:
    env=dict(os.environ); entries=[str(BIN)]
    lib=RESOURCE_ROOT/'lib'
    if lib.is_dir(): entries.append(str(lib))
    entries.append(env.get('PATH','')); env['PATH']=os.pathsep.join(entries)
    if lib.is_dir() and os.name!='nt':
        key='DYLD_LIBRARY_PATH' if sys_platform()=='darwin' else 'LD_LIBRARY_PATH'; env[key]=os.pathsep.join([str(lib),env.get(key,'')])
    tess=RESOURCE_ROOT/'share'/'tessdata'
    if tess.is_dir(): env['TESSDATA_PREFIX']=str(tess)
    return env


def sys_platform():
    import sys; return sys.platform


def minimal_pdf(path: Path) -> None:
    objects=[
        b'1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n',
        b'2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n',
        b'3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] /Contents 4 0 R >>\nendobj\n',
        b'4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n',
    ]
    data=bytearray(b'%PDF-1.4\n'); offsets=[0]
    for obj in objects: offsets.append(len(data)); data.extend(obj)
    xref=len(data); data.extend(f'xref\n0 {len(objects)+1}\n'.encode()); data.extend(b'0000000000 65535 f \n')
    for offset in offsets[1:]: data.extend(f'{offset:010d} 00000 n \n'.encode())
    data.extend(f'trailer\n<< /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()); path.write_bytes(data)


def run(exe: Path, args: list[str], env: dict[str,str], timeout=60) -> None:
    result=subprocess.run([str(exe),*args],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=timeout)
    if result.returncode!=0: raise RuntimeError(f'{exe.name} exited {result.returncode}: {(result.stdout or "")[-1000:]}')


def main() -> None:
    parser=argparse.ArgumentParser(); parser.add_argument('--mode',choices=['optional','core','full'],default='optional'); args=parser.parse_args()
    meta=json.loads(META.read_text()); available={item['name']:BIN/item['name'] for item in meta.get('engines',[])}
    env=env_for_pack(); failures=[]; tested=[]
    with tempfile.TemporaryDirectory(prefix='fileflow-engine-functional-') as tmp:
        root=Path(tmp); png=root/'input.png'; png.write_bytes(PNG_1X1); ppm=root/'input.ppm'; ppm.write_text('P3\n2 2\n255\n255 0 0  0 255 0\n0 0 255  255 255 255\n'); ocr_pgm=root/'ocr.pgm'; ocr_pgm.write_bytes(b'P5\n240 80\n255\n' + bytes([255]) * (240 * 80)); pdf=root/'input.pdf'; minimal_pdf(pdf); text=root/'input.txt'; text.write_text('FileFlow engine functional test\n'); md=root/'input.md'; md.write_text('# FileFlow\n\nfunctional engine test\n')
        def exe(engine,name):
            variants=[name,name+'.exe']; return next((available[v] for v in variants if v in available),None)
        tests=[]
        ffmpeg=exe('ffmpeg','ffmpeg'); ffprobe=exe('ffmpeg','ffprobe')
        if ffmpeg: tests.append(('ffmpeg',lambda: run(ffmpeg,['-y','-loglevel','error','-i',str(png),str(root/'ffmpeg.bmp')],env)))
        if ffprobe: tests.append(('ffprobe',lambda: run(ffprobe,['-v','error','-show_format',str(png)],env)))
        magick=exe('imagemagick','magick')
        if magick: tests.append(('imagemagick',lambda: run(magick,[str(png),str(root/'magick.bmp')],env)))
        vips=exe('vips','vips')
        if vips: tests.append(('vips',lambda: run(vips,['copy',str(ppm),str(root/'vips.ppm')],env)))
        qpdf=exe('qpdf','qpdf')
        if qpdf: tests.append(('qpdf',lambda: run(qpdf,['--check',str(pdf)],env)))
        seven=exe('archive','7zz')
        if seven:
            def seven_test():
                archive=root/'fixture.7z'; out=root/'7z-out'; out.mkdir(); run(seven,['a','-bd','-y',str(archive),str(text)],env); run(seven,['x','-bd','-y',f'-o{out}',str(archive)],env); assert (out/text.name).is_file()
            tests.append(('archive',seven_test))
        zstd=exe('zstd','zstd')
        if zstd:
            def zstd_test(): run(zstd,['-q','-f',str(text),'-o',str(root/'input.zst')],env); run(zstd,['-q','-d','-f',str(root/'input.zst'),'-o',str(root/'zstd.txt')],env)
            tests.append(('zstd',zstd_test))
        lz4=exe('lz4','lz4')
        if lz4:
            def lz4_test(): run(lz4,['-q','-f',str(text),str(root/'input.lz4')],env); run(lz4,['-q','-d','-f',str(root/'input.lz4'),str(root/'lz4.txt')],env)
            tests.append(('lz4',lz4_test))
        exiftool=exe('metadata','exiftool')
        if exiftool: tests.append(('metadata',lambda: run(exiftool,['-j',str(png)],env)))
        tesseract=exe('tesseract','tesseract')
        if tesseract: tests.append(('tesseract',lambda: run(tesseract,[str(ocr_pgm),'stdout','--psm','7'],env)))
        pdftotext=exe('poppler','pdftotext'); pdftoppm=exe('poppler','pdftoppm')
        if pdftotext: tests.append(('pdftotext',lambda: run(pdftotext,[str(pdf),str(root/'poppler.txt')],env)))
        if pdftoppm: tests.append(('pdftoppm',lambda: run(pdftoppm,['-singlefile','-f','1','-l','1',str(pdf),str(root/'poppler')],env)))
        gs=exe('ghostscript','gs')
        if gs: tests.append(('ghostscript',lambda: run(gs,['-q','-dBATCH','-dNOPAUSE','-sDEVICE=pdfwrite',f'-sOutputFile={root/"gs.pdf"}',str(pdf)],env)))
        pandoc=exe('pandoc','pandoc')
        if pandoc: tests.append(('pandoc',lambda: run(pandoc,[str(md),'-o',str(root/'pandoc.html')],env)))
        soffice=exe('office','soffice')
        if soffice: tests.append(('office',lambda: run(soffice,['--headless','--convert-to','pdf','--outdir',str(root),str(text)],env,120)))
        ocr=exe('ocr','ocrmypdf')
        if ocr: tests.append(('ocrmypdf',lambda: run(ocr,['--skip-text','--output-type','pdf',str(pdf),str(root/'ocr.pdf')],env,120)))
        img2pdf=exe('img2pdf','img2pdf')
        if img2pdf: tests.append(('img2pdf',lambda: run(img2pdf,[str(png),'-o',str(root/'img2pdf.pdf')],env)))
        for name,test in tests:
            try: test(); tested.append(name); print(f'[OK] functional {name}')
            except Exception as error: failures.append(f'{name}: {error}')
    print(f'functional-tested {len(tested)} staged engine command(s)')
    if failures:
        print('functional engine failures:'); [print('  -',f) for f in failures]; raise SystemExit(2)


if __name__=='__main__': main()
