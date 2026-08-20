#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "release/engines/manifest.json"
RESOURCE_ROOT = ROOT / "src-tauri/resources/engines"
BIN = RESOURCE_ROOT / "bin"
META = ROOT / "src-tauri/resources/engine-pack.json"


def env_for_pack() -> dict[str, str]:
    env = dict(os.environ)
    env["PATH"] = os.pathsep.join([str(BIN), env.get("PATH", "")])
    lib = RESOURCE_ROOT / "lib"
    if lib.is_dir():
        key = "DYLD_LIBRARY_PATH" if sys_platform() == "darwin" else "LD_LIBRARY_PATH"
        if sys_platform() != "win32":
            env[key] = os.pathsep.join([str(lib), env.get(key, "")])
    tessdata = RESOURCE_ROOT / "share" / "tessdata"
    if tessdata.is_dir():
        env["TESSDATA_PREFIX"] = str(tessdata)
    return env


def sys_platform() -> str:
    import sys
    return sys.platform


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["optional", "core", "full"], default="optional")
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text())
    meta = json.loads(META.read_text())
    staged = {(item["engine"], item["name"]) for item in meta.get("engines", [])}
    environment = env_for_pack()
    failures: list[str] = []
    count = 0

    for engine in manifest["engines"]:
        probe_args = engine.get("probeArgs", ["--version"])
        for executable in engine["executables"]:
            variants = [executable, f"{executable}.exe"]
            name = next((name for name in variants if (engine["id"], name) in staged), None)
            if name is None:
                continue
            path = BIN / name
            count += 1
            try:
                result = subprocess.run(
                    [str(path), *probe_args],
                    env=environment,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                    timeout=20,
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

    print(f"smoke-tested {count} staged engine executable(s) ({args.mode})")
    if failures:
        print("broken staged engines:")
        for failure in failures:
            print(f"  - {failure}")
        raise SystemExit(2)


if __name__ == "__main__":
    main()
