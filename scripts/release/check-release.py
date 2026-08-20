#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load_json(rel: str) -> dict:
    return json.loads((ROOT / rel).read_text())


parser = argparse.ArgumentParser()
parser.add_argument("--allow-dirty", action="store_true")
args = parser.parse_args()

versions = {
    "package.json": load_json("package.json")["version"],
    "frontend/package.json": load_json("frontend/package.json")["version"],
    "src-tauri/tauri.conf.json": load_json("src-tauri/tauri.conf.json")["version"],
}
cargo_text = (ROOT / "src-tauri/Cargo.toml").read_text()
match = re.search(r'(?m)^version = "([^"]+)"$', cargo_text)
versions["src-tauri/Cargo.toml"] = match.group(1) if match else "?"
if len(set(versions.values())) != 1:
    raise SystemExit(f"version mismatch: {versions}")
version = next(iter(versions.values()))
if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
    raise SystemExit(f"invalid release version: {version}")
print("version", version)

required_files = [
    "src-tauri/tauri.windows.conf.json",
    "src-tauri/tauri.macos.conf.json",
    "src-tauri/tauri.linux.conf.json",
    "release/engines/manifest.json",
    "scripts/release/stage-engines.py",
    "scripts/release/smoke-engines.py",
    ".github/workflows/release.yml",
]
for rel in required_files:
    if not (ROOT / rel).is_file():
        raise SystemExit(f"missing release file: {rel}")

frontend = load_json("frontend/package.json")
for dep in ["@tauri-apps/plugin-updater", "@tauri-apps/plugin-process"]:
    if dep not in frontend.get("dependencies", {}):
        raise SystemExit(f"missing frontend dependency: {dep}")
for crate in ["tauri-plugin-updater", "tauri-plugin-process"]:
    if not re.search(rf"(?m)^{re.escape(crate)}\s*=", cargo_text):
        raise SystemExit(f"missing Rust dependency: {crate}")

capabilities = load_json("src-tauri/capabilities/default.json").get("permissions", [])
for permission in [
    "core:window:allow-show",
    "core:window:allow-set-focus",
    "updater:default",
    "process:default",
]:
    if permission not in capabilities:
        raise SystemExit(f"missing capability: {permission}")

base = load_json("src-tauri/tauri.conf.json")
windows = load_json("src-tauri/tauri.windows.conf.json")
macos = load_json("src-tauri/tauri.macos.conf.json")
linux = load_json("src-tauri/tauri.linux.conf.json")
if windows.get("bundle", {}).get("targets") != ["nsis", "msi"]:
    raise SystemExit("Windows release must produce NSIS + MSI")
if macos.get("bundle", {}).get("targets") != ["app", "dmg"]:
    raise SystemExit("macOS release must produce APP + DMG")
if set(linux.get("bundle", {}).get("targets", [])) != {"deb", "appimage", "rpm"}:
    raise SystemExit("Linux release must produce DEB + AppImage + RPM")
if base.get("app", {}).get("windows", [{}])[0].get("visible") is not False:
    raise SystemExit("main window must remain hidden until auth bootstrap is resolved")

manifest = load_json("release/engines/manifest.json")
ids = [entry.get("id") for entry in manifest.get("engines", [])]
if len(ids) != len(set(ids)) or not ids:
    raise SystemExit("engine manifest ids must be unique and non-empty")
for entry in manifest["engines"]:
    if entry.get("tier") not in {"core", "extended", "optional"}:
        raise SystemExit(f"invalid engine tier: {entry}")
    if not entry.get("executables") or not entry.get("probeArgs"):
        raise SystemExit(f"engine manifest entry is incomplete: {entry.get('id')}")

subprocess.run(["git", "diff", "--check"], cwd=ROOT, check=True)
if not args.allow_dirty:
    dirty = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()
    if dirty:
        raise SystemExit("working tree is not clean")
print("release metadata OK")
