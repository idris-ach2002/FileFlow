#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "release/engines/manifest.json"
DEFAULT_RESOURCE_ROOT = ROOT / "src-tauri/resources/engines"
DEFAULT_META = ROOT / "src-tauri/resources/engine-pack.json"


def clean_environment(resource_root: Path) -> dict[str, str]:
    keep = (
        "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR", "TMP", "TEMP",
        "SystemRoot", "WINDIR", "USERPROFILE", "APPDATA", "LOCALAPPDATA",
    )
    env = {key: value for key in keep if (value := os.environ.get(key))}
    bin_dir = resource_root / "bin"
    if os.name == "nt":
        root = env.get("SystemRoot", r"C:\Windows")
        base = [str(Path(root) / "System32"), root]
    else:
        base = ["/usr/bin", "/bin"]
    env["PATH"] = os.pathsep.join([str(bin_dir), *base])
    env["PYTHONNOUSERSITE"] = "1"
    return env


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["optional", "core", "full"], default="optional")
    parser.add_argument("--engine-root", type=Path, default=DEFAULT_RESOURCE_ROOT)
    parser.add_argument("--metadata", type=Path, default=DEFAULT_META)
    args = parser.parse_args()

    manifest = json.loads(MANIFEST.read_text())
    meta = json.loads(args.metadata.read_text())
    bin_dir = args.engine_root / "bin"
    staged = {(item["engine"], item["name"]) for item in meta.get("engines", [])}
    environment = clean_environment(args.engine_root)
    failures: list[str] = []
    count = 0

    for engine in manifest["engines"]:
        probe_args = engine.get("probeArgs", ["--version"])
        for executable in engine["executables"]:
            variants = [executable, f"{executable}.exe"]
            name = next((name for name in variants if (engine["id"], name) in staged), None)
            if name is None:
                continue
            path = bin_dir / name
            count += 1
            try:
                result = subprocess.run(
                    [str(path), *probe_args],
                    env=environment,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                    timeout=30,
                )
            except Exception as error:
                failures.append(f"{engine['id']}:{name}: {error}")
                continue
            output = (result.stdout or "").strip().splitlines()
            summary = output[0][:160] if output else f"exit {result.returncode}"
            if result.returncode != 0:
                failures.append(f"{engine['id']}:{name}: exit {result.returncode}: {summary}")
            else:
                print(f"[OK] {engine['id']}:{name} — {summary}")

    print(f"smoke-tested {count} staged engine executable(s) ({args.mode}) in clean-host environment")
    if failures:
        print("broken staged engines:")
        for failure in failures:
            print(f"  - {failure}")
        raise SystemExit(2)


if __name__ == "__main__":
    main()
