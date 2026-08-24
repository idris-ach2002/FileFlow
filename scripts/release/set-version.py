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

workspace = ROOT / "Cargo.toml"
workspace_text = workspace.read_text()
workspace_start = workspace_text.find("[workspace.package]")
if workspace_start < 0:
    raise SystemExit("unable to find [workspace.package] in Cargo.toml")
workspace_end = workspace_text.find("\n[", workspace_start + 1)
if workspace_end < 0:
    workspace_end = len(workspace_text)
workspace_section = workspace_text[workspace_start:workspace_end]
workspace_section, count = re.subn(
    r'(?m)^version\s*=\s*"[^"]+"$',
    f'version = "{version}"',
    workspace_section,
    count=1,
)
if count != 1:
    raise SystemExit("unable to update Cargo.toml [workspace.package] version")
workspace.write_text(
    workspace_text[:workspace_start] + workspace_section + workspace_text[workspace_end:]
)

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

workspace_package_names = set()
for manifest in [ROOT / "src-tauri/Cargo.toml", *sorted((ROOT / "crates").glob("**/Cargo.toml"))]:
    manifest_text = manifest.read_text()
    match = re.search(r'(?m)^name\s*=\s*"([^"]+)"$', manifest_text)
    if match:
        workspace_package_names.add(match.group(1))

lock = ROOT / "Cargo.lock"
lock_text = lock.read_text()
for package_name in sorted(workspace_package_names):
    pattern = rf'(name = "{re.escape(package_name)}"\nversion = ")[^"]+("\n)'
    lock_text, count = re.subn(pattern, rf'\g<1>{version}\g<2>', lock_text, count=1)
    if count != 1:
        raise SystemExit(f"unable to update Cargo.lock version for {package_name}")
lock.write_text(lock_text)
print(f"FileFlow version -> {version}")
