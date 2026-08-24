#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TAURI = ROOT / "src-tauri"
out = TAURI / "tauri.release.conf.json"


def deep_merge(base: dict, overlay: dict) -> dict:
    result = copy.deepcopy(base)
    for key, value in overlay.items():
        if isinstance(value, dict) and isinstance(result.get(key), dict):
            result[key] = deep_merge(result[key], value)
        else:
            result[key] = copy.deepcopy(value)
    return result


def host_target() -> str:
    try:
        return subprocess.check_output(["rustc", "--print", "host-tuple"], text=True).strip()
    except Exception:
        verbose = subprocess.check_output(["rustc", "-vV"], text=True)
        for line in verbose.splitlines():
            if line.startswith("host: "):
                return line.removeprefix("host: ").strip()
        raise SystemExit("unable to determine Rust host target")


def platform_config(target: str) -> Path:
    if "windows" in target: return TAURI / "tauri.windows.conf.json"
    if "apple-darwin" in target: return TAURI / "tauri.macos.conf.json"
    if "linux" in target: return TAURI / "tauri.linux.conf.json"
    raise SystemExit(f"unsupported release target: {target}")


def require(name: str) -> str:
    value=os.environ.get(name,"").strip()
    if not value: raise SystemExit(f"strict release requires {name}")
    return value


parser = argparse.ArgumentParser()
parser.add_argument("--target", default=None)
parser.add_argument("--strict", action="store_true", help="require production updater signing configuration")
parser.add_argument(
    "--require-platform-signing",
    action="store_true",
    help="also require native macOS/Windows code-signing credentials",
)
args = parser.parse_args()
target = args.target or host_target()

base = json.loads((TAURI / "tauri.conf.json").read_text())
platform = json.loads(platform_config(target).read_text())
bundle = deep_merge(base.get("bundle", {}), platform.get("bundle", {}))

pubkey = os.environ.get("TAURI_UPDATER_PUBKEY", "").strip()
endpoint = os.environ.get("FILEFLOW_UPDATE_ENDPOINT", "").strip()
private_key = os.environ.get("TAURI_SIGNING_PRIVATE_KEY", "").strip()
if args.strict:
    pubkey=require("TAURI_UPDATER_PUBKEY")
    endpoint=require("FILEFLOW_UPDATE_ENDPOINT")
    private_key=require("TAURI_SIGNING_PRIVATE_KEY")
    require("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
updater_enabled = bool(pubkey and endpoint and private_key)
bundle["createUpdaterArtifacts"] = updater_enabled

if "apple-darwin" in target:
    macos = bundle.setdefault("macOS", {})
    assert isinstance(macos, dict)
    signing_identity = os.environ.get("APPLE_SIGNING_IDENTITY", "").strip()
    if args.require_platform_signing and (not signing_identity or signing_identity == "-"):
        raise SystemExit("platform-signed macOS release requires a Developer ID Application identity")
    notarization = [
        os.environ.get("APPLE_ID", "").strip(),
        os.environ.get("APPLE_PASSWORD", "").strip(),
        os.environ.get("APPLE_TEAM_ID", "").strip(),
    ]
    if any(notarization) and not all(notarization):
        raise SystemExit("macOS notarization credentials are incomplete")
    macos["signingIdentity"] = signing_identity or "-"

config: dict[str, object] = {"bundle": bundle}
config["plugins"] = {"updater": ({
    "pubkey": pubkey,
    "endpoints": [endpoint],
    "windows": {"installMode": "passive"},
} if updater_enabled else {"pubkey": "", "endpoints": []})}

thumbprint = os.environ.get("WINDOWS_CERTIFICATE_THUMBPRINT", "").strip()
if "windows" in target:
    if args.require_platform_signing and not thumbprint:
        raise SystemExit("platform-signed Windows release requires WINDOWS_CERTIFICATE_THUMBPRINT")
    if thumbprint:
        windows = bundle.setdefault("windows", {})
        assert isinstance(windows, dict)
        windows["certificateThumbprint"] = thumbprint
        windows["digestAlgorithm"] = "sha256"
        timestamp = os.environ.get("WINDOWS_TIMESTAMP_URL", "").strip()
        if args.require_platform_signing and not timestamp:
            raise SystemExit("strict Windows release requires WINDOWS_TIMESTAMP_URL")
        if timestamp: windows["timestampUrl"] = timestamp

out.write_text(json.dumps(config, indent=2) + "\n")
print(f"target: {target}")
print(f"mode: {'strict-production' if args.strict else 'development/package-smoke'}")
print(f"updater artifacts: {'enabled' if updater_enabled else 'disabled'}")
if "apple-darwin" in target:
    identity = bundle.get("macOS", {}).get("signingIdentity", "?")
    print(f"macOS signing: {identity}")
print(f"windows signing: {'configured' if thumbprint and 'windows' in target else 'not configured'}")
print(out)
