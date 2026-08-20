#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

CHUNK = 80 * 1024 * 1024
TARGETS = {
    "x86_64-unknown-linux-gnu": (
        "linux", "x64", "distribution/linux-x64", ".AppImage"
    ),
    "aarch64-unknown-linux-gnu": (
        "linux", "arm64", "distribution/linux-arm64", ".AppImage"
    ),
    "aarch64-apple-darwin": (
        "macos", "arm64", "distribution/macos-arm64", ".dmg"
    ),
    "x86_64-apple-darwin": (
        "macos", "x64", "distribution/macos-x64", ".dmg"
    ),
    "x86_64-pc-windows-msvc": (
        "windows", "x64", "distribution/windows-x64", ".exe"
    ),
}


def run(
    *args: str,
    cwd: Path | None = None,
    capture: bool = False,
):
    print("+", " ".join(args))
    return subprocess.run(
        args,
        cwd=str(cwd) if cwd else None,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture else None,
    )


def choose(root: Path, suffix: str) -> Path:
    matches = sorted(
        path
        for path in root.rglob("*")
        if path.is_file()
        and path.name.lower().endswith(suffix.lower())
    )
    if len(matches) != 1:
        raise SystemExit(
            f"expected exactly one {suffix} below {root}, "
            f"found {matches}"
        )
    return matches[0]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(
            lambda: handle.read(1024 * 1024),
            b"",
        ):
            digest.update(chunk)
    return digest.hexdigest()


def verify_full_engines(
    repo: Path,
    target: str,
) -> tuple[str, int, int]:
    release_manifest = json.loads(
        (repo / "release/engines/manifest.json").read_text()
    )
    metadata_path = (
        repo / "src-tauri/resources/engine-pack.json"
    )
    if not metadata_path.is_file():
        raise SystemExit(
            "refusing distribution publish: "
            "staged engine-pack.json is missing"
        )
    metadata = json.loads(metadata_path.read_text())

    if metadata.get("target") != target:
        raise SystemExit(
            "refusing distribution publish: engine target mismatch"
        )
    if metadata.get("flavor") != "full":
        raise SystemExit(
            "refusing distribution publish: engine flavor is not FULL"
        )
    if not metadata.get("hardened"):
        raise SystemExit(
            "refusing distribution publish: "
            "engine runtime was not hardened"
        )

    pack_version = str(
        metadata.get("packVersion", "")
    ).strip()
    expected_pack_version = str(
        release_manifest.get("packVersion", "")
    ).strip()
    if (
        not pack_version
        or pack_version != expected_pack_version
    ):
        raise SystemExit(
            "refusing distribution publish: "
            "engine pack version mismatch"
        )

    staged = {
        (
            str(item.get("engine", "")),
            str(item.get("name", "")),
        )
        for item in metadata.get("engines", [])
    }

    missing: list[str] = []
    expected = 0
    for engine in release_manifest["engines"]:
        for executable in engine["executables"]:
            expected += 1
            variants = [executable, f"{executable}.exe"]
            if not any(
                (engine["id"], name) in staged
                for name in variants
            ):
                missing.append(
                    f"{engine['id']}:{executable}"
                )

    if missing:
        raise SystemExit(
            "refusing distribution publish: "
            "missing FULL engines: "
            + ", ".join(missing)
        )

    if len(staged) != expected:
        raise SystemExit(
            "refusing distribution publish: "
            f"staged executable count {len(staged)} "
            f"!= expected {expected}"
        )

    print(
        f"[engines] publish gate OK target={target} "
        f"pack={pack_version} "
        f"executables={len(staged)}/{expected}"
    )
    return pack_version, len(staged), expected


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument(
        "--channel",
        choices=["candidate", "production"],
        default="candidate",
    )
    args = parser.parse_args()

    if args.target not in TARGETS:
        raise SystemExit(
            f"unsupported target {args.target}"
        )

    platform, arch, branch, suffix = TARGETS[args.target]
    root = Path(args.root).resolve()
    package = choose(root, suffix)

    repo = Path(
        run(
            "git",
            "rev-parse",
            "--show-toplevel",
            capture=True,
        ).stdout.strip()
    )
    source = run(
        "git",
        "rev-parse",
        "HEAD",
        capture=True,
        cwd=repo,
    ).stdout.strip()
    version = str(
        json.loads(
            (repo / "src-tauri/tauri.conf.json").read_text()
        )["version"]
    )

    (
        engine_pack_version,
        engine_count,
        expected_engine_count,
    ) = verify_full_engines(repo, args.target)

    package_sha = sha256(package)
    package_size = package.stat().st_size

    tmp = Path(
        tempfile.mkdtemp(
            prefix="fileflow-git-payload-"
        )
    )
    worktree = tmp / "worktree"

    try:
        run(
            "git",
            "worktree",
            "add",
            "--detach",
            str(worktree),
            "HEAD",
            cwd=repo,
        )
        run(
            "git",
            "switch",
            "--orphan",
            f"payload-{platform}-{arch}",
            cwd=worktree,
        )

        for child in list(worktree.iterdir()):
            if child.name == ".git":
                continue
            if child.is_dir():
                shutil.rmtree(child)
            else:
                child.unlink()

        payload = worktree / "payload"
        payload.mkdir()
        parts = 0
        with package.open("rb") as source_handle:
            while True:
                data = source_handle.read(CHUNK)
                if not data:
                    break
                (
                    payload / f"part-{parts:04d}"
                ).write_bytes(data)
                parts += 1

        values = {
            "VERSION": version,
            "SOURCE_SHA": source,
            "TARGET": args.target,
            "PLATFORM": platform,
            "ARCH": arch,
            "CHANNEL": args.channel,
            "PACKAGE_NAME": package.name,
            "PACKAGE_SHA256": package_sha,
            "PACKAGE_SIZE": str(package_size),
            "CHUNK_COUNT": str(parts),
            "ENGINE_MODE": "full",
            "ENGINE_PACK_VERSION": engine_pack_version,
            "ENGINE_EXECUTABLE_COUNT": str(engine_count),
            "ENGINE_EXPECTED_EXECUTABLE_COUNT": str(
                expected_engine_count
            ),
        }

        (worktree / "manifest.env").write_text(
            "".join(
                f"{key}={value}\n"
                for key, value in values.items()
            ),
            encoding="ascii",
        )
        (worktree / "README.txt").write_text(
            "FileFlow binary transport branch.\n"
            "Publication is fail-closed and requires "
            "a validated FULL engine pack.\n",
            encoding="utf-8",
        )

        run("git", "add", "-A", cwd=worktree)
        run(
            "git",
            "-c",
            "user.name=github-actions[bot]",
            "-c",
            "user.email="
            "41898282+github-actions[bot]"
            "@users.noreply.github.com",
            "commit",
            "-m",
            f"dist({platform}): "
            f"FileFlow {version} {arch} {source[:12]}",
            cwd=worktree,
        )
        run(
            "git",
            "push",
            "--force",
            "origin",
            f"HEAD:refs/heads/{branch}",
            cwd=worktree,
        )
        print(
            f"[OK] {branch} chunks={parts} "
            f"sha256={package_sha} "
            f"engines=full/{engine_count}"
        )
    finally:
        try:
            run(
                "git",
                "worktree",
                "remove",
                "--force",
                str(worktree),
                cwd=repo,
            )
        except Exception:
            pass
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
