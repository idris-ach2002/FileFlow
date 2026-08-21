#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load_json(rel: str) -> dict:
    return json.loads((ROOT / rel).read_text(encoding="utf-8"))


def require_tokens(rel: str, tokens: list[str]) -> str:
    text = (ROOT / rel).read_text(encoding="utf-8")
    for token in tokens:
        if token not in text:
            raise SystemExit(f"{rel} missing release invariant: {token}")
    return text


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--allow-dirty", action="store_true")
    args = parser.parse_args()

    cargo_text = (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
    workspace_cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    lock_text = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    workspace_section = workspace_cargo.split("[workspace.package]", 1)[1].split("\n[", 1)[0]
    versions = {
        "package.json": load_json("package.json")["version"],
        "frontend/package.json": load_json("frontend/package.json")["version"],
        "src-tauri/tauri.conf.json": load_json("src-tauri/tauri.conf.json")["version"],
        "src-tauri/Cargo.toml": re.search(r'(?m)^version = "([^"]+)"$', cargo_text).group(1),
        "Cargo.toml [workspace.package]": re.search(r'(?m)^version\s*=\s*"([^"]+)"$', workspace_section).group(1),
        "Cargo.lock": re.search(r'name = "fileflow-desktop"\nversion = "([^"]+)"', lock_text).group(1),
    }
    if len(set(versions.values())) != 1:
        raise SystemExit(f"version mismatch: {versions}")
    version = next(iter(versions.values()))
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise SystemExit(f"invalid release version: {version}")

    required = [
        "install.sh", "install.ps1",
        "scripts/runtime/install-dependencies.sh", "scripts/runtime/install-dependencies.ps1",
        "scripts/runtime/doctor.sh", "scripts/runtime/doctor.ps1",
        "scripts/runtime/stage-bundled-runtime.py", "scripts/runtime/smoke-bundled-runtime.py",
        "src-tauri/runtime/runtime-manifest.json",
        "scripts/release/generate-release-config.py", "scripts/release/publish-git-payload.py",
        "scripts/release/smoke-packaged-app.mjs", "scripts/release/collect-artifacts.mjs",
        "src-tauri/tauri.windows.conf.json", "src-tauri/tauri.macos.conf.json", "src-tauri/tauri.linux.conf.json",
        ".github/workflows/ci.yml", ".github/workflows/native-linux.yml",
        ".github/workflows/native-macos.yml", ".github/workflows/native-windows.yml",
        ".github/workflows/release-linux.yml", ".github/workflows/release-macos.yml",
        ".github/workflows/release-windows.yml",
    ]
    for rel in required:
        if not (ROOT / rel).is_file():
            raise SystemExit(f"missing release file: {rel}")

    # Legacy engine-pack machinery remains forbidden. The new runtime is staged
    # directly from each native runner and embedded as a Tauri resource, avoiding
    # a second incompatible AppImage/conda loader environment.
    forbidden_paths = [
        "release/engines", ".github/workflows/engine-packs.yml",
        "scripts/release/build-native-engine-pack.py", "scripts/release/stage-engines.py",
        "scripts/release/harden-engine-pack.py", "scripts/release/validate-engine-pack.py",
        "scripts/release/fetch-engine-pack.py", "scripts/release/fetch-libreoffice-runtime.py",
        "scripts/release/smoke-packaged-engines.py",
    ]
    for rel in forbidden_paths:
        if (ROOT / rel).exists():
            raise SystemExit(f"legacy engine-pack infrastructure must stay removed: {rel}")

    package = load_json("package.json")
    if package.get("engines", {}).get("node") != ">=22.22.3 <23 || >=24.15.0 <25 || >=26 <27":
        raise SystemExit("Node support range must stay aligned with Angular 22 supported majors")
    if 'channel = "1.97.1"' not in (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"):
        raise SystemExit("Rust toolchain must be pinned to 1.97.1")

    base = load_json("src-tauri/tauri.conf.json")
    if "resources" in base.get("bundle", {}):
        raise SystemExit("base Tauri bundle keeps platform runtime resources in platform configs")
    if base.get("app", {}).get("windows", [{}])[0].get("visible") is not False:
        raise SystemExit("main window must remain hidden until auth bootstrap is resolved")
    if load_json("src-tauri/tauri.windows.conf.json").get("bundle", {}).get("targets") != ["nsis", "msi"]:
        raise SystemExit("Windows release must produce NSIS + MSI")
    if load_json("src-tauri/tauri.macos.conf.json").get("bundle", {}).get("targets") != ["app", "dmg"]:
        raise SystemExit("macOS release must produce APP + DMG")
    if set(load_json("src-tauri/tauri.linux.conf.json").get("bundle", {}).get("targets", [])) != {"deb", "appimage", "rpm"}:
        raise SystemExit("Linux release must produce DEB + AppImage + RPM")
    for rel in ["src-tauri/tauri.linux.conf.json", "src-tauri/tauri.macos.conf.json", "src-tauri/tauri.windows.conf.json"]:
        resources = load_json(rel).get("bundle", {}).get("resources", [])
        if "runtime/**/*" in resources:
            raise SystemExit(f"{rel} must not embed generated conversion engines")

    unix_installer = require_tokens("install.sh", [
        "git fetch", "RUNTIME_MODE", "system-managed", "Dépendances système FileFlow",
    ])
    windows_installer = require_tokens("install.ps1", [
        "git fetch", "RUNTIME_MODE", "system-managed", "moteurs de conversion Windows",
    ])
    require_tokens("scripts/runtime/install-dependencies.sh", [
        "apt-get", "dnf", "zypper", "pacman", "brew", "pipx", "trying next source",
    ])
    require_tokens("scripts/runtime/install-dependencies.ps1", [
        "winget", "choco", "scoop", "pipx", "trying next source",
    ])
    require_tokens("scripts/release/publish-git-payload.py", ['"RUNTIME_MODE": "system-managed"'])
    require_tokens("crates/fileflow-engine/src/lib.rs", [
        "FILEFLOW_ENGINE_PATH", "set_bundled_runtime_root", "runtime-manifest.json",
        "/opt/homebrew/bin", "Microsoft/WinGet/Links", ".local/bin",
    ])
    require_tokens("crates/fileflow-executor/src/lib.rs", [
        "configure_external_command", "PYTHONHOME", "LD_LIBRARY_PATH", "APPDIR",
    ])

    forbidden_workflow_tokens = [
        "micromamba", "engine-certify", "stage-engines.py", "engine-pack",
        "fetch-libreoffice-runtime.py", "smoke-packaged-engines.py", "FILEFLOW_ENGINE_PACK",
    ]
    for rel in [
        ".github/workflows/native-linux.yml", ".github/workflows/native-macos.yml", ".github/workflows/native-windows.yml",
        ".github/workflows/release-linux.yml", ".github/workflows/release-macos.yml", ".github/workflows/release-windows.yml",
    ]:
        text = (ROOT / rel).read_text(encoding="utf-8")
        for token in forbidden_workflow_tokens:
            if token.lower() in text.lower():
                raise SystemExit(f"{rel} still contains legacy engine-factory token: {token}")
        if "tauri build" not in text:
            raise SystemExit(f"{rel} must build the FileFlow application")
        if "stage-bundled-runtime.py" in text or "smoke-bundled-runtime.py" in text:
            raise SystemExit(f"{rel} must not build conversion-engine runtimes in CI")

    if "ENGINE_PACK_" in unix_installer or "ENGINE_PACK_" in windows_installer:
        raise SystemExit("installer manifests must no longer depend on engine pack metadata")

    subprocess.run(["git", "diff", "--check"], cwd=ROOT, check=True)
    if not args.allow_dirty:
        dirty = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()
        if dirty:
            raise SystemExit("working tree is not clean")
    print(f"release metadata OK; FileFlow {version}; runtime engines=system-managed")


if __name__ == "__main__":
    main()
