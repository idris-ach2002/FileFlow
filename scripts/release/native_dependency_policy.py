#!/usr/bin/env python3
from __future__ import annotations

import struct
from pathlib import Path

# FileFlow's native runtime policy is deliberately conservative: only ABI
# components guaranteed by the target operating system remain external. Every
# other native dependency must be present in the engine pack.
LINUX_BASE_ABI = frozenset({
    "libc.so.6",
    "libm.so.6",
    "libpthread.so.0",
    "libdl.so.2",
    "librt.so.1",
    "libutil.so.1",
    "libresolv.so.2",
    "libanl.so.1",
    "ld-linux-x86-64.so.2",
    "ld-linux-aarch64.so.1",
})

MACOS_SYSTEM_PREFIXES = ("/System/Library/", "/usr/lib/")

WINDOWS_SYSTEM_DLLS = frozenset({
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "cfgmgr32.dll", "combase.dll",
    "comctl32.dll", "comdlg32.dll", "crypt32.dll", "d2d1.dll", "d3d11.dll", "dbghelp.dll",
    "dnsapi.dll", "dwmapi.dll", "dxgi.dll", "gdi32.dll", "gdi32full.dll", "imm32.dll",
    "iphlpapi.dll", "kernel32.dll", "kernelbase.dll", "msimg32.dll", "msvcrt.dll",
    "ncrypt.dll", "netapi32.dll", "normaliz.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll",
    "powrprof.dll", "profapi.dll", "psapi.dll", "rpcrt4.dll", "sechost.dll", "setupapi.dll",
    "shell32.dll", "shlwapi.dll", "sspicli.dll", "ucrtbase.dll", "urlmon.dll", "user32.dll",
    "userenv.dll", "usp10.dll", "version.dll", "winhttp.dll", "wininet.dll", "winmm.dll",
    "winspool.drv", "ws2_32.dll", "wtsapi32.dll", "oleacc.dll", "propsys.dll",
    "wintrust.dll", "cryptbase.dll", "cryptsp.dll", "msasn1.dll", "nsi.dll", "secur32.dll",
})


def is_linux_system_dependency(name: str) -> bool:
    token = name.strip().split(" (", 1)[0]
    return token in LINUX_BASE_ABI or token in {"linux-vdso.so.1", "linux-gate.so.1"}


def is_macos_system_dependency(name: str) -> bool:
    return name.startswith(MACOS_SYSTEM_PREFIXES)


def is_windows_system_dependency(name: str) -> bool:
    lowered = name.strip().lower()
    return lowered in WINDOWS_SYSTEM_DLLS or lowered.startswith(("api-ms-win-", "ext-ms-win-"))


def expected_elf_machine(target: str) -> int | None:
    if target.startswith("x86_64-"):
        return 62  # EM_X86_64
    if target.startswith("aarch64-"):
        return 183  # EM_AARCH64
    return None


def elf_machine(path: Path) -> int | None:
    try:
        with path.open("rb") as handle:
            header = handle.read(20)
        if len(header) < 20 or header[:4] != b"\x7fELF":
            return None
        endian = "little" if header[5] == 1 else "big" if header[5] == 2 else None
        if endian is None:
            return None
        return int.from_bytes(header[18:20], endian)
    except OSError:
        return None


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


def expected_pe_machine(target: str) -> int | None:
    if target.startswith("x86_64-"):
        return 0x8664
    if target.startswith("aarch64-"):
        return 0xAA64
    return None
