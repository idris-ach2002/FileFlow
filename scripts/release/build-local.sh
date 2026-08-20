#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
TARGET="${1:-$(rustc --print host-tuple)}"
MODE="${FILEFLOW_ENGINE_MODE:-optional}"
python3 scripts/release/stage-engines.py --target "$TARGET" --mode "$MODE"
python3 scripts/release/smoke-engines.py --mode "$MODE"
python3 scripts/release/generate-release-config.py --target "$TARGET"
corepack enable
pnpm install --frozen-lockfile
pnpm run verify
node scripts/release/validate-frontend-dist.mjs
case "$TARGET" in
  *apple-darwin) BUNDLES="app,dmg";;
  *windows-msvc) BUNDLES="nsis,msi";;
  *linux-gnu) BUNDLES="deb,appimage,rpm";;
  *) echo "unsupported target: $TARGET" >&2; exit 2;;
esac
pnpm tauri build --target "$TARGET" --bundles "$BUNDLES" --config src-tauri/tauri.release.conf.json
