#!/usr/bin/env python3
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", sys.argv[1]):
    raise SystemExit("usage: set-version.py X.Y.Z")

version = sys.argv[1]
for rel in ["package.json", "frontend/package.json", "src-tauri/tauri.conf.json"]:
    path = ROOT / rel
    data = json.loads(path.read_text())
    data["version"] = version
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")

cargo = ROOT / "src-tauri/Cargo.toml"
text = cargo.read_text()
text, count = re.subn(
    r'(?m)^version = "[^"]+"$',
    f'version = "{version}"',
    text,
    count=1,
)
if count != 1:
    raise SystemExit("unable to update src-tauri/Cargo.toml version")
cargo.write_text(text)

lock = ROOT / "Cargo.lock"
lock_text = lock.read_text()
pattern = r'(name = "fileflow-desktop"\nversion = ")[^"]+("\n)'
lock_text, count = re.subn(pattern, rf'\g<1>{version}\g<2>', lock_text, count=1)
if count != 1:
    raise SystemExit("unable to update Cargo.lock FileFlow version")
lock.write_text(lock_text)
print(f"FileFlow version -> {version}")
