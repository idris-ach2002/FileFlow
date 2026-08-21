#!/usr/bin/env python3
from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def executable(name: str) -> str:
    found = shutil.which(name)
    if found:
        return found
    if os.name == "nt":
        found = shutil.which(f"{name}.cmd") or shutil.which(f"{name}.exe")
        if found:
            return found
    raise SystemExit(f"[FAIL] command not found: {name}")


def run(args: list[str], *, quiet: bool = False) -> None:
    kwargs = {"cwd": ROOT, "check": True}
    if quiet:
        kwargs.update(stdout=subprocess.DEVNULL)
    subprocess.run(args, **kwargs)


print("== FileFlow release bootstrap ==")
node = executable("node")
pnpm = executable("pnpm")
cargo = executable("cargo")

print("Node :", subprocess.check_output([node, "--version"], text=True).strip())
print("pnpm :", subprocess.check_output([pnpm, "--version"], text=True).strip())
print("Cargo:", subprocess.check_output([cargo, "--version"], text=True).strip())

print("\n1/4 Synchronize pnpm lockfile")
run([pnpm, "install", "--no-frozen-lockfile"])

print("\n2/4 Synchronize Cargo.lock")
run([cargo, "metadata", "--format-version", "1"], quiet=True)
run([cargo, "metadata", "--locked", "--format-version", "1"], quiet=True)

print("\n3/4 Validate release metadata")
python = sys.executable
run([python, "scripts/release/check-release.py", "--allow-dirty"])

print("\n4/4 Run the cross-platform quality gate")
run([pnpm, "run", "verify"])

print("\nFILEFLOW RELEASE BOOTSTRAP PASSED")
print("Lockfiles are synchronized. Review and commit pnpm-lock.yaml and Cargo.lock.")
