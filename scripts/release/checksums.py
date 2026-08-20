#!/usr/bin/env python3
import argparse
import hashlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

parser = argparse.ArgumentParser()
parser.add_argument("--target", required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()

roots = [ROOT / "target" / args.target / "release" / "bundle", ROOT / "target" / "release" / "bundle"]
files = []
for bundle_root in roots:
    if not bundle_root.is_dir():
        continue
    files.extend(
        path
        for path in bundle_root.rglob("*")
        if path.is_file() and path.suffix.lower() in {".dmg", ".msi", ".exe", ".deb", ".rpm", ".appimage", ".gz", ".sig", ".json"}
    )

unique = sorted(set(files))
if not unique:
    raise SystemExit("no release artifacts found for checksum generation")

lines = []
for path in unique:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    lines.append(f"{digest}  {path.name}")
args.output.write_text("\n".join(lines) + "\n")
print(args.output)
