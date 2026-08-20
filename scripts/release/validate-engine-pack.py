#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
ENGINE_ROOT=ROOT/'src-tauri/resources/engines'
BIN=ENGINE_ROOT/'bin'; LIB=ENGINE_ROOT/'lib'; META=ROOT/'src-tauri/resources/engine-pack.json'


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)


def files() -> list[Path]:
    # FULL packs contain a private relocated runtime under share/runtime and a
    # private LibreOffice tree under share/libreoffice. Validate their native
    # binaries too; wrappers in bin/ must not hide broken dependencies.
    result=[]
    for p in ENGINE_ROOT.rglob('*'):
        if not p.is_file():
            continue
        lower=p.name.lower()
        executable=bool(p.stat().st_mode & 0o111)
        native_suffix=lower.endswith(('.exe','.dll','.dylib','.so')) or '.so.' in lower
        if executable or native_suffix:
            result.append(p)
    return result


def file_output(path: Path) -> str:
    tool=shutil.which('file')
    return run(tool,str(path)).stdout if tool else ''


def expected_arch(target: str) -> tuple[str,...]:
    if target.startswith('aarch64-'): return ('arm64','aarch64','ARM aarch64')
    if target.startswith('x86_64-'): return ('x86_64','x86-64','x86_64')
    return ()


def is_native(path: Path, family: str) -> bool:
    if family=='windows': return path.suffix.lower() in {'.exe','.dll'}
    kind=file_output(path)
    return 'Mach-O' in kind if family=='macos' else 'ELF' in kind


def validate_macos(paths: list[Path], target: str, require_signature: bool) -> list[str]:
    failures=[]; expected=expected_arch(target)
    for path in paths:
        if not is_native(path,'macos'): continue
        info=file_output(path)
        if expected and not any(token in info for token in expected): failures.append(f'{path}: wrong architecture: {info.strip()}')
        deps=run('otool','-L',str(path))
        if deps.returncode!=0: failures.append(f'{path}: otool failed: {deps.stdout.strip()}')
        for line in deps.stdout.splitlines()[1:]:
            dep=line.strip().split(' (',1)[0]
            if dep.startswith(('/opt/homebrew/','/usr/local/','/tmp/','/private/tmp/','/Users/','/home/runner/')):
                failures.append(f'{path}: non-portable dependency {dep}')
        if require_signature:
            result=run('codesign','--verify','--strict','--verbose=2',str(path))
            if result.returncode!=0: failures.append(f'{path}: invalid code signature: {result.stdout.strip()}')
    return failures


def validate_linux(paths: list[Path], target: str) -> list[str]:
    failures=[]; expected=expected_arch(target)
    for path in paths:
        if not is_native(path,'linux'): continue
        info=file_output(path)
        if expected and not any(token in info for token in expected): failures.append(f'{path}: wrong architecture: {info.strip()}')
        dyn=run('readelf','-d',str(path))
        dynamic = dyn.returncode == 0 and 'There is no dynamic section' not in dyn.stdout
        if dynamic and LIB.is_dir() and '$ORIGIN' not in dyn.stdout:
            failures.append(f'{path}: missing $ORIGIN runtime search path')
        ldd=run('ldd',str(path))
        if ldd.returncode==0 and 'not found' in ldd.stdout:
            failures.append(f'{path}: unresolved shared library: {ldd.stdout.strip()}')
        if re.search(r'=>\s+/(home|opt|usr/local)/',ldd.stdout):
            failures.append(f'{path}: depends on non-system host path: {ldd.stdout.strip()}')
    return failures


def pe_headers(path: Path) -> tuple[int, int, int, list[tuple[bytes, int, int, int]]] | None:
    """Return machine, PE offset, optional-header offset and section table."""
    try:
        data = path.read_bytes()
        if len(data) < 0x40 or data[:2] != b'MZ': return None
        pe_offset = struct.unpack_from('<I', data, 0x3c)[0]
        if data[pe_offset:pe_offset+4] != b'PE\0\0': return None
        coff = pe_offset + 4
        machine, sections, _, _, _, optional_size, _ = struct.unpack_from('<HHIIIHH', data, coff)
        optional = coff + 20
        section_offset = optional + optional_size
        table=[]
        for index in range(sections):
            off=section_offset + index*40
            if off+40 > len(data): return None
            name=data[off:off+8].rstrip(b'\0')
            virtual_size, virtual_address, raw_size, raw_pointer = struct.unpack_from('<IIII',data,off+8)
            table.append((name, virtual_address, max(virtual_size,raw_size), raw_pointer))
        return machine, pe_offset, optional, table
    except (OSError, struct.error, IndexError):
        return None


def pe_machine(path: Path) -> int | None:
    headers=pe_headers(path)
    return headers[0] if headers else None


def pe_imports(path: Path) -> list[str]:
    """Read PE import-table DLL names without depending on Visual Studio dumpbin."""
    try:
        data=path.read_bytes(); headers=pe_headers(path)
        if not headers: return []
        _, _, optional, sections=headers
        magic=struct.unpack_from('<H',data,optional)[0]
        data_directory = optional + (112 if magic == 0x20B else 96 if magic == 0x10B else -1)
        if data_directory < optional: return []
        import_rva, import_size=struct.unpack_from('<II',data,data_directory+8)  # directory index 1
        if not import_rva or not import_size: return []
        def rva_to_offset(rva: int) -> int | None:
            for _, virtual_address, span, raw_pointer in sections:
                if virtual_address <= rva < virtual_address + span:
                    return raw_pointer + (rva - virtual_address)
            return None
        descriptor=rva_to_offset(import_rva)
        if descriptor is None: return []
        names=[]
        for _ in range(4096):
            if descriptor+20 > len(data): break
            original, timestamp, chain, name_rva, thunk=struct.unpack_from('<IIIII',data,descriptor)
            if original==timestamp==chain==name_rva==thunk==0: break
            name_offset=rva_to_offset(name_rva)
            if name_offset is None: break
            end=data.find(b'\0',name_offset)
            if end < 0: break
            names.append(data[name_offset:end].decode('ascii','replace'))
            descriptor += 20
        return names
    except (OSError, struct.error, IndexError):
        return []


WINDOWS_SYSTEM_DLLS = {
    'advapi32.dll','bcrypt.dll','bcryptprimitives.dll','cfgmgr32.dll','combase.dll',
    'comctl32.dll','comdlg32.dll','crypt32.dll','d2d1.dll','d3d11.dll','dbghelp.dll',
    'dnsapi.dll','dwmapi.dll','dxgi.dll','gdi32.dll','gdi32full.dll','imm32.dll',
    'iphlpapi.dll','kernel32.dll','kernelbase.dll','msimg32.dll','msvcrt.dll',
    'ncrypt.dll','netapi32.dll','normaliz.dll','ntdll.dll','ole32.dll','oleaut32.dll',
    'powrprof.dll','profapi.dll','psapi.dll','rpcrt4.dll','sechost.dll','setupapi.dll',
    'shell32.dll','shlwapi.dll','sspicli.dll','ucrtbase.dll','urlmon.dll','user32.dll',
    'userenv.dll','usp10.dll','version.dll','winhttp.dll','wininet.dll','winmm.dll',
    'winspool.drv','ws2_32.dll','wtsapi32.dll','oleacc.dll','propsys.dll',
}

def validate_windows(paths: list[Path], target: str, require_signature: bool) -> list[str]:
    failures=[]
    expected=0x8664 if target.startswith('x86_64-') else 0xAA64
    packed={path.name.lower() for path in paths if path.suffix.lower()=='.dll'}
    for path in paths:
        if not is_native(path,'windows'): continue
        machine=pe_machine(path)
        if machine is None: continue
        if machine!=expected: failures.append(f'{path}: PE machine 0x{machine:04x}, expected 0x{expected:04x}')
        system_root=Path(os.environ.get('SystemRoot', r'C:\Windows'))
        system_dirs=(system_root/'System32', system_root/'SysWOW64')
        for dependency in pe_imports(path):
            name=dependency.lower()
            if name in WINDOWS_SYSTEM_DLLS or name.startswith(('api-ms-win-','ext-ms-win-')):
                continue
            if any((directory/dependency).is_file() for directory in system_dirs):
                continue
            if name not in packed:
                failures.append(f'{path}: imported DLL is neither bundled nor provided by Windows: {dependency}')
        if require_signature:
            escaped=str(path).replace("'","''")
            cmd=f"$s=Get-AuthenticodeSignature -LiteralPath '{escaped}'; if ($s.Status -ne 'Valid') {{ Write-Error $s.Status; exit 2 }}"
            result=run('powershell','-NoProfile','-Command',cmd)
            if result.returncode!=0: failures.append(f'{path}: invalid Authenticode signature: {result.stdout.strip()}')
    return failures


def main() -> None:
    parser=argparse.ArgumentParser(); parser.add_argument('--target',required=True); parser.add_argument('--require-signature',action='store_true'); args=parser.parse_args()
    if not META.is_file(): raise SystemExit('missing staged engine metadata')
    meta=json.loads(META.read_text())
    if meta.get('target')!=args.target: raise SystemExit('staged engine metadata target mismatch')
    staged=[BIN/item['name'] for item in meta.get('engines',[])]
    for path in staged:
        if not path.is_file(): raise SystemExit(f'missing staged engine: {path}')
    paths=files()
    family='macos' if 'apple-darwin' in args.target else ('windows' if 'windows' in args.target else 'linux')
    failures = validate_macos(paths,args.target,args.require_signature) if family=='macos' else (validate_windows(paths,args.target,args.require_signature) if family=='windows' else validate_linux(paths,args.target))
    if failures:
        print('native engine validation failed:')
        for failure in failures: print('  -',failure)
        raise SystemExit(2)
    print(f'validated {len(paths)} native engine pack file(s) for {args.target}')


if __name__=='__main__': main()
