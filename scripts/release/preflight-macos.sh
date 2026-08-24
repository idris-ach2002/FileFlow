#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "[preflight-macos] This preflight must run on macOS." >&2
  exit 2
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
cd "$ROOT"

for command_name in node pnpm rustup cargo codesign hdiutil lipo tar; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "[preflight-macos] Missing command: $command_name" >&2
    exit 2
  fi
done

PRIVATE_KEY=${TAURI_SIGNING_PRIVATE_KEY:-$HOME/.tauri/fileflow.key}
if [[ ! -s "$PRIVATE_KEY" ]]; then
  echo "[preflight-macos] Missing updater private key: $PRIVATE_KEY" >&2
  exit 2
fi

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
  read -r -s -p "Updater signing password: " TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  printf '\n'
fi
if [[ -z "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" ]]; then
  echo "[preflight-macos] The updater signing password cannot be empty." >&2
  exit 2
fi

TAURI_UPDATER_PUBKEY=$(node -p "require('./src-tauri/tauri.conf.json').plugins.updater.pubkey || ''")
FILEFLOW_UPDATE_ENDPOINT=$(node -p "require('./src-tauri/tauri.conf.json').plugins.updater.endpoints?.[0] || ''")
if [[ -z "$TAURI_UPDATER_PUBKEY" || -z "$FILEFLOW_UPDATE_ENDPOINT" ]]; then
  echo "[preflight-macos] Updater pubkey or endpoint is not configured." >&2
  exit 2
fi

export TAURI_SIGNING_PRIVATE_KEY="$PRIVATE_KEY"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
export TAURI_UPDATER_PUBKEY
export FILEFLOW_UPDATE_ENDPOINT

unset APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID

RELEASE_CONFIG="$ROOT/src-tauri/tauri.release.conf.json"
SAVED_CONFIG=""
CONFIG_EXISTED=0
CURRENT_MOUNT=""

if [[ -e "$RELEASE_CONFIG" ]]; then
  CONFIG_EXISTED=1
  SAVED_CONFIG=$(mktemp "${TMPDIR:-/tmp}/fileflow-release-config.XXXXXX")
  cp "$RELEASE_CONFIG" "$SAVED_CONFIG"
fi

cleanup() {
  if [[ -n "$CURRENT_MOUNT" && -d "$CURRENT_MOUNT" ]]; then
    hdiutil detach "$CURRENT_MOUNT" >/dev/null 2>&1 || true
    rmdir "$CURRENT_MOUNT" >/dev/null 2>&1 || true
  fi
  if [[ "$CONFIG_EXISTED" -eq 1 ]]; then
    cp "$SAVED_CONFIG" "$RELEASE_CONFIG"
    rm -f "$SAVED_CONFIG"
  else
    rm -f "$RELEASE_CONFIG"
  fi
  unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
}
trap cleanup EXIT INT TERM

rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm install --frozen-lockfile

KEY_PROBE_DIR=$(mktemp -d "${TMPDIR:-/tmp}/fileflow-updater-key.XXXXXX")
printf 'FileFlow updater key validation\n' > "$KEY_PROBE_DIR/probe.txt"
if ! env -u TAURI_SIGNING_PRIVATE_KEY pnpm exec tauri signer sign \
  --private-key-path "$PRIVATE_KEY" \
  --password "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" \
  "$KEY_PROBE_DIR/probe.txt" >/dev/null 2>&1; then
  rm -rf "$KEY_PROBE_DIR"
  echo "[preflight-macos] The supplied password does not unlock the updater private key." >&2
  echo "[preflight-macos] No macOS build was started." >&2
  exit 2
fi
rm -rf "$KEY_PROBE_DIR"
echo "[preflight-macos] Updater private key and password verified."

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  echo "[preflight-macos] Building $target"

  python3 scripts/release/generate-release-config.py --target "$target" --strict
  node -e '
    const config = require("./src-tauri/tauri.release.conf.json");
    const identity = config.bundle?.macOS?.signingIdentity;
    if (identity !== "-") throw new Error(`expected ad-hoc identity -, received ${identity}`);
    if (!config.bundle?.createUpdaterArtifacts) throw new Error("updater artifacts are disabled");
    console.log("[preflight-macos] config OK: ad-hoc signing + updater artifacts");
  '

  BUNDLE_ROOT="$ROOT/target/$target/release/bundle"
  rm -rf "$BUNDLE_ROOT/macos" "$BUNDLE_ROOT/dmg"

  CI=true pnpm exec tauri build \
    --target "$target" \
    --bundles app,dmg \
    --config src-tauri/tauri.release.conf.json

  APP="$BUNDLE_ROOT/macos/FileFlow.app"
  ARCHIVE="$BUNDLE_ROOT/macos/FileFlow.app.tar.gz"
  SIGNATURE="$ARCHIVE.sig"
  DMG=""
  for candidate in "$BUNDLE_ROOT"/dmg/*.dmg; do
    if [[ -f "$candidate" ]]; then
      DMG="$candidate"
      break
    fi
  done

  if [[ ! -d "$APP" || ! -s "$ARCHIVE" || ! -s "$SIGNATURE" || -z "$DMG" ]]; then
    echo "[preflight-macos] Missing APP, DMG or signed updater artifacts for $target" >&2
    exit 2
  fi

  codesign --verify --deep --strict --verbose=2 "$APP"
  tar -tzf "$ARCHIVE" >/dev/null
  hdiutil verify "$DMG"

  EXECUTABLE="$APP/Contents/MacOS/fileflow-desktop"
  ARCHITECTURES=$(lipo -archs "$EXECUTABLE")
  case "$target:$ARCHITECTURES" in
    aarch64-apple-darwin:*arm64*) ;;
    x86_64-apple-darwin:*x86_64*) ;;
    *)
      echo "[preflight-macos] Unexpected executable architecture: $target -> $ARCHITECTURES" >&2
      exit 2
      ;;
  esac

  CURRENT_MOUNT=$(mktemp -d "${TMPDIR:-/tmp}/fileflow-dmg.XXXXXX")
  hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$CURRENT_MOUNT" >/dev/null
  if [[ ! -d "$CURRENT_MOUNT/FileFlow.app" ]]; then
    echo "[preflight-macos] FileFlow.app is missing from $DMG" >&2
    exit 2
  fi
  codesign --verify --deep --strict --verbose=2 "$CURRENT_MOUNT/FileFlow.app"
  hdiutil detach "$CURRENT_MOUNT" >/dev/null
  rmdir "$CURRENT_MOUNT"
  CURRENT_MOUNT=""

  node scripts/release/validate-distribution.mjs --target "$target"
  node scripts/release/smoke-packaged-app.mjs --target "$target"

  echo "[preflight-macos] PASS $target"
done

echo "[preflight-macos] PASS: ARM64 + Intel APP/DMG, code signatures, updater artifacts and packaged runtimes"
