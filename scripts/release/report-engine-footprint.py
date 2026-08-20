#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ENGINE_ROOT = ROOT / "src-tauri/resources/engines"
OUT = ROOT / "release/engines/out"


def bytes_below(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    if not path.is_dir():
        return 0
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def human(size: int) -> str:
    value = float(size)
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if value < 1024.0 or unit == "TiB":
            return f"{value:.2f} {unit}"
        value /= 1024.0
    return f"{size} B"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    args = parser.parse_args()

    family = "macos" if "apple-darwin" in args.target else ("windows" if "windows" in args.target else "linux")
    sections = [
        ("engines-total", ENGINE_ROOT),
        ("runtime", ENGINE_ROOT / "share/runtime"),
        ("libreoffice", ENGINE_ROOT / "share/libreoffice"),
        (f"vendor-{family}", ENGINE_ROOT / f"share/vendor/{family}"),
    ]
    print(f"[footprint] target={args.target}")
    for name, path in sections:
        size = bytes_below(path)
        print(f"[footprint] {name}={human(size)} bytes={size}")

    provenance = ENGINE_ROOT / "provenance/runtime-packages.json"
    if provenance.is_file():
        raw = json.loads(provenance.read_text(encoding="utf-8"))
        vendored = raw.get("vendoredHostLibraries", [])
        total = sum(int(item.get("size", 0)) for item in vendored)
        print(f"[footprint] vendored-host-libraries={len(vendored)} size={human(total)} bytes={total}")

    archives = sorted(OUT.glob(f"fileflow-engines-*-{args.target}.tar.gz"))
    for archive in archives:
        size = archive.stat().st_size
        print(f"[footprint] archive={archive.name} size={human(size)} bytes={size}")


if __name__ == "__main__":
    main()
