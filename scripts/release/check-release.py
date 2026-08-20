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


def require_tokens(rel: str, tokens: list[str]) -> str:
    text = (ROOT / rel).read_text()
    for token in tokens:
        if token not in text:
            raise SystemExit(f"{rel} missing release invariant: {token}")
    return text


def main() -> None:
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

    workspace_cargo = (ROOT / "Cargo.toml").read_text()
    workspace_section = workspace_cargo.split("[workspace.package]", 1)[1].split("\n[", 1)[0] if "[workspace.package]" in workspace_cargo else ""
    workspace_match = re.search(r'(?m)^version\s*=\s*"([^"]+)"$', workspace_section)
    versions["Cargo.toml [workspace.package]"] = workspace_match.group(1) if workspace_match else "?"

    lock_text = (ROOT / "Cargo.lock").read_text()
    match = re.search(r'name = "fileflow-desktop"\nversion = "([^"]+)"', lock_text)
    versions["Cargo.lock"] = match.group(1) if match else "?"
    if len(set(versions.values())) != 1:
        raise SystemExit(f"version mismatch: {versions}")
    version = next(iter(versions.values()))
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise SystemExit(f"invalid release version: {version}")
    print("version", version)

    required_files = [
        "src-tauri/tauri.windows.conf.json", "src-tauri/tauri.macos.conf.json", "src-tauri/tauri.linux.conf.json",
        "release/engines/manifest.json", "release/engines/python-requirements.txt",
        "scripts/ci/verify-platform.mjs", "scripts/ci/run-stress.mjs",
        "scripts/release/build-native-engine-pack.py", "scripts/release/stage-engines.py",
        "scripts/release/stage-local-engine-pack.py", "scripts/release/harden-engine-pack.py",
        "scripts/release/validate-engine-pack.py", "scripts/release/functional-engine-tests.py",
        "scripts/release/test-native-engine-tooling.py", "scripts/release/fetch-engine-pack.py",
        "scripts/release/make-engine-pack.py", "scripts/release/smoke-engines.py",
        "scripts/release/smoke-packaged-engines.py", "scripts/release/smoke-packaged-app.mjs",
        "scripts/release/publish-git-payload.py", "scripts/release/collect-artifacts.mjs",
        "scripts/ci/windows-preflight.ps1", "install.sh", "install.ps1",
        "scripts/release/normalize-artifacts.mjs", "scripts/release/generate-updater-manifest.mjs",
        "scripts/release/test-release-tooling.mjs", "scripts/release/verify-release.mjs",
        ".github/workflows/ci.yml", ".github/workflows/engine-packs.yml",
        ".github/workflows/native-linux.yml", ".github/workflows/native-macos.yml", ".github/workflows/native-windows.yml",
        ".github/workflows/release-linux.yml", ".github/workflows/release-macos.yml", ".github/workflows/release-windows.yml",
    ]
    for rel in required_files:
        if not (ROOT / rel).is_file():
            raise SystemExit(f"missing release file: {rel}")

    package = load_json("package.json")
    if package.get("engines", {}).get("node") != ">=22.22.3 <23 || >=24.15.0 <25 || >=26 <27":
        raise SystemExit("Node support range must stay aligned with Angular 22 supported majors")
    if 'channel = "1.97.1"' not in (ROOT / "rust-toolchain.toml").read_text():
        raise SystemExit("Rust toolchain must be pinned to 1.97.1")
    frontend = load_json("frontend/package.json")
    for dep in ["@tauri-apps/plugin-updater", "@tauri-apps/plugin-process"]:
        if dep not in frontend.get("dependencies", {}):
            raise SystemExit(f"missing frontend dependency: {dep}")
    for crate in ["tauri-plugin-updater", "tauri-plugin-process"]:
        if not re.search(rf"(?m)^{re.escape(crate)}\s*=", cargo_text):
            raise SystemExit(f"missing Rust dependency: {crate}")

    capabilities = load_json("src-tauri/capabilities/default.json").get("permissions", [])
    for permission in ["core:window:allow-show", "core:window:allow-set-focus", "updater:default", "process:default"]:
        if permission not in capabilities:
            raise SystemExit(f"missing capability: {permission}")
    base = load_json("src-tauri/tauri.conf.json")
    windows = load_json("src-tauri/tauri.windows.conf.json")
    macos = load_json("src-tauri/tauri.macos.conf.json")
    linux = load_json("src-tauri/tauri.linux.conf.json")
    if windows.get("bundle", {}).get("targets") != ["nsis", "msi"]:
        raise SystemExit("Windows release must produce NSIS + MSI")
    if windows.get("bundle", {}).get("windows", {}).get("staticVCRuntime") is not True:
        raise SystemExit("Windows release must bundle the VC runtime")
    if macos.get("bundle", {}).get("targets") != ["app", "dmg"]:
        raise SystemExit("macOS release must produce APP + DMG")
    if set(linux.get("bundle", {}).get("targets", [])) != {"deb", "appimage", "rpm"}:
        raise SystemExit("Linux release must produce DEB + AppImage + RPM")
    if base.get("app", {}).get("windows", [{}])[0].get("visible") is not False:
        raise SystemExit("main window must remain hidden until auth bootstrap is resolved")

    manifest = load_json("release/engines/manifest.json")
    pack_version = str(manifest.get("packVersion", ""))
    if not re.fullmatch(r"\d+\.\d+\.\d+", pack_version):
        raise SystemExit("engine manifest requires immutable semantic packVersion")
    ids = [entry.get("id") for entry in manifest.get("engines", [])]
    if len(ids) != len(set(ids)) or not ids:
        raise SystemExit("engine manifest ids must be unique and non-empty")
    for entry in manifest["engines"]:
        if entry.get("tier") not in {"core", "extended", "optional"}:
            raise SystemExit(f"invalid engine tier: {entry}")
        if not entry.get("executables") or not entry.get("probeArgs"):
            raise SystemExit(f"engine manifest entry is incomplete: {entry.get('id')}")

    requirements = [line.strip() for line in (ROOT / "release/engines/python-requirements.txt").read_text().splitlines() if line.strip() and not line.lstrip().startswith("#")]
    if not requirements or any("==" not in line for line in requirements):
        raise SystemExit("direct portable Python engine requirements must be exactly pinned")

    hardener = require_tokens("scripts/release/harden-engine-pack.py", [
        "safe_existing_linux_rpaths", "resolve_linux_needed", "ambiguous internal ELF dependency",
        "engine-runtime-paths.txt", "ambiguous internal Mach-O dependency", "refresh_metadata()",
    ])
    validator = require_tokens("scripts/release/validate-engine-pack.py", [
        "parse_ldd_unresolved", "clean_environment", "--audit-full", "bundled dependency is unreachable via $ORIGIN RPATH",
    ])
    factory = require_tokens("scripts/release/build-native-engine-pack.py", [
        "Zero-dependency host contract", 'PATH="$BIN_DIR:$RUNTIME/bin:', "engine-runtime-paths.txt", "MAGICK_CONFIGURE_PATH", "GS_LIB",
    ])
    if "${PATH:-}" in factory:
        raise SystemExit("Unix engine wrappers must not inherit arbitrary host PATH")
    if "def windows_harden() -> None:\n    #" in hardener and "return" in hardener.split("def windows_harden() -> None:", 1)[1].split("\ndef ", 1)[0]:
        raise SystemExit("Windows hardening must not be a no-op")
    if "is_linux_virtual_dependency(dep)" in validator.split("def validate_macos", 1)[1].split("def ", 1)[0]:
        raise SystemExit("Linux VDSO filtering must never be wired into macOS validation")

    engine_workflow = require_tokens(".github/workflows/engine-packs.yml", [
        "Build + certify engines", "build-native-engine-pack.py", "functional-engine-tests.py",
        "Re-stage archive and prove it is self-contained", "engines-v${PACK_VERSION}",
    ])
    if "source_url_template" in engine_workflow:
        raise SystemExit("engine-packs workflow must build packs itself; external candidate URLs are forbidden")

    linux_native = require_tokens(".github/workflows/native-linux.yml", [
        "pull_request:", "engine-certify:", "package-smoke:", "needs: [native, engine-certify]",
        "needs.native.result == 'success'", "needs.engine-certify.result == 'success'",
        "stage-local-engine-pack.py", "smoke-packaged-engines.py", "--audit-full",
    ])
    if "always() && !cancelled()" in linux_native:
        raise SystemExit("Linux package smoke must not run after failed engine certification")

    for rel in [".github/workflows/native-macos.yml", ".github/workflows/native-windows.yml"]:
        require_tokens(rel, [
            "pull_request:", "engine-certify:", "package-smoke:", "needs: [native, engine-certify]",
            "always() && !cancelled()", "stage-local-engine-pack.py", "smoke-packaged-engines.py", "--audit-full",
        ])

    for rel in [".github/workflows/release-linux.yml", ".github/workflows/release-macos.yml", ".github/workflows/release-windows.yml"]:
        release_text = require_tokens(rel, ["FILEFLOW_ENGINE_MODE: full", "smoke-packaged-engines.py"])
        if "vars.FILEFLOW_ENGINE_MODE" in release_text:
            raise SystemExit(f"{rel} must certify the FULL engine tier; configurable core mode is forbidden")

    installer_sh = require_tokens("install.sh", [
        "Client prerequisite: Git only.", "git fetch", '"$ENGINE_MODE" = "full"', "ENGINE_EXPECTED_EXECUTABLE_COUNT",
    ])
    installer_ps = require_tokens("install.ps1", [
        "git fetch", "ENGINE_MODE'] -ne 'full", "ENGINE_EXPECTED_EXECUTABLE_COUNT", "actuellement certifié x64 uniquement",
    ])
    for forbidden in ["apt-get install", "brew install", "pnpm install", "npm install", "cargo build", "pip install", "micromamba", "docker ", "gh "]:
        if forbidden in installer_sh.lower() or forbidden in installer_ps.lower():
            raise SystemExit(f"Git-only installer must not provision build/runtime dependency: {forbidden}")

    release_windows = require_tokens(".github/workflows/release-windows.yml", [
        "Authenticode-sign Windows engine binaries", "Validate signed Windows engines", "--require-signature",
    ])
    signed_index = release_windows.index("Authenticode-sign Windows engine binaries")
    after_sign = release_windows[signed_index:]
    if "harden-engine-pack.py" in after_sign:
        raise SystemExit("Windows release mutates engine pack after Authenticode signing")

    require_tokens(".github/workflows/release-macos.yml", [
        "harden-engine-pack.py", "--require-signature", "Build signed/notarized macOS bundle",
    ])
    for rel in [".github/workflows/release-linux.yml", ".github/workflows/release-macos.yml", ".github/workflows/release-windows.yml"]:
        require_tokens(rel, ["git merge-base --is-ancestor", "FILEFLOW_ENGINE_PACK_URL_TEMPLATE", "fetch-engine-pack.py"])

    subprocess.run(["git", "diff", "--check"], cwd=ROOT, check=True)
    if not args.allow_dirty:
        dirty = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()
        if dirty:
            raise SystemExit("working tree is not clean")
    print(f"release metadata OK; engine pack {pack_version}")


if __name__ == "__main__":
    main()
