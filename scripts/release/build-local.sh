#!/usr/bin/env bash
set -euo pipefail
TARGET="$(rustc --print host-tuple 2>/dev/null || rustc -vV | sed -n 's/^host: //p')"
python3 scripts/release/generate-release-config.py --target "$TARGET"
pnpm exec tauri build --target "$TARGET" --config src-tauri/tauri.release.conf.json "$@"
