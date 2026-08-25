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
SETUP_RELEASE_CONFIG="$ROOT/setup-tauri/tauri.release.conf.json"
SETUP_TARGET_ROOT="$ROOT/target/fileflow-setup"
export FILEFLOW_SETUP_TARGET_DIR="$SETUP_TARGET_ROOT"
SAVED_CONFIG=""
SAVED_SETUP_CONFIG=""
CONFIG_EXISTED=0
SETUP_CONFIG_EXISTED=0
CURRENT_MOUNT=""

if [[ -e "$RELEASE_CONFIG" ]]; then
  CONFIG_EXISTED=1
  SAVED_CONFIG=$(mktemp "${TMPDIR:-/tmp}/fileflow-release-config.XXXXXX")
  cp "$RELEASE_CONFIG" "$SAVED_CONFIG"
fi
if [[ -e "$SETUP_RELEASE_CONFIG" ]]; then
  SETUP_CONFIG_EXISTED=1
  SAVED_SETUP_CONFIG=$(mktemp "${TMPDIR:-/tmp}/fileflow-setup-release-config.XXXXXX")
  cp "$SETUP_RELEASE_CONFIG" "$SAVED_SETUP_CONFIG"
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
  if [[ "$SETUP_CONFIG_EXISTED" -eq 1 ]]; then
    cp "$SAVED_SETUP_CONFIG" "$SETUP_RELEASE_CONFIG"
    rm -f "$SAVED_SETUP_CONFIG"
  else
    rm -f "$SETUP_RELEASE_CONFIG"
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
  SETUP_BUNDLE_ROOT="$SETUP_TARGET_ROOT/$target/release/bundle"
  rm -rf "$BUNDLE_ROOT/macos" "$BUNDLE_ROOT/dmg"
  rm -rf "$SETUP_BUNDLE_ROOT/macos" "$SETUP_BUNDLE_ROOT/dmg" "$SETUP_BUNDLE_ROOT/setup-cli"

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

  CI=true CARGO_TARGET_DIR="$SETUP_TARGET_ROOT" node scripts/setup/run-tauri.mjs build \
    --target "$target" \
    --bundles app,dmg \
    --config tauri.release.conf.json
  CARGO_TARGET_DIR="$SETUP_TARGET_ROOT" \
    cargo build --release -p fileflow-setup --bin fileflow-setup-cli --target "$target"
  node scripts/release/package-setup-cli.mjs --target "$target"

  SETUP_APP="$SETUP_BUNDLE_ROOT/macos/FileFlowSetup.app"
  SETUP_DMG=""
  for candidate in "$SETUP_BUNDLE_ROOT"/dmg/*FileFlowSetup*.dmg; do
    if [[ -f "$candidate" ]]; then SETUP_DMG="$candidate"; break; fi
  done
  if [[ ! -d "$SETUP_APP" || -z "$SETUP_DMG" ]]; then
    echo "[preflight-macos] Missing FileFlow Setup APP/DMG for $target" >&2
    exit 2
  fi
  codesign --verify --deep --strict --verbose=2 "$SETUP_APP"
  SETUP_EMBEDDED_CLI="$SETUP_APP/Contents/MacOS/fileflow-setup-cli"
  if [[ ! -x "$SETUP_EMBEDDED_CLI" ]]; then
    echo "[preflight-macos] Embedded Setup CLI missing from $target" >&2
    exit 2
  fi
  codesign --verify --strict --verbose=2 "$SETUP_EMBEDDED_CLI"
  hdiutil verify "$SETUP_DMG"
  SETUP_ARCHITECTURES=$(lipo -archs "$SETUP_APP/Contents/MacOS/fileflow-setup")
  SETUP_CLI_ARCHITECTURES=$(lipo -archs "$SETUP_EMBEDDED_CLI")
  case "$target:$SETUP_ARCHITECTURES" in
    aarch64-apple-darwin:*arm64*) ;;
    x86_64-apple-darwin:*x86_64*) ;;
    *) echo "[preflight-macos] Unexpected Setup architecture: $target -> $SETUP_ARCHITECTURES" >&2; exit 2 ;;
  esac
  case "$target:$SETUP_CLI_ARCHITECTURES" in
    aarch64-apple-darwin:*arm64*) ;;
    x86_64-apple-darwin:*x86_64*) ;;
    *) echo "[preflight-macos] Unexpected embedded Setup CLI architecture: $target -> $SETUP_CLI_ARCHITECTURES" >&2; exit 2 ;;
  esac
  node scripts/release/validate-distribution.mjs --target "$target" --require-setup
  node scripts/release/smoke-packaged-setup.mjs --target "$target"
  "$SETUP_TARGET_ROOT/$target/release/fileflow-setup-cli" doctor --dry-run --json >/dev/null

  echo "[preflight-macos] PASS $target — FileFlow + Setup + CLI"
done

echo "[preflight-macos] PASS: ARM64 + Intel FileFlow/Setup/CLI, DMG, signatures, updater artifacts and packaged runtimes"
