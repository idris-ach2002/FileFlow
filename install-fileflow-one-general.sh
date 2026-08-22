#!/usr/bin/env bash
set -Eeuo pipefail

REPO="${FILEFLOW_REPO:-idris-ach2002/FileFlow}"
REF="${FILEFLOW_REF:-main}"
KEEP_TEMP="${FILEFLOW_KEEP_TEMP:-0}"

OS=""
ARCH=""
TARGET=""
DIST_BRANCH=""
TMP_ROOT=""
SRC_DIR=""
PAYLOAD_REF=""

log()  { printf '\033[1;34m[FileFlow]\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m[ OK ]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[WARN]\033[0m %s\n' "$*" >&2; }
fail() { printf '\033[1;31m[FAIL]\033[0m %s\n' "$*" >&2; exit 1; }

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n "${TMP_ROOT:-}" && -d "$TMP_ROOT" ]]; then
    if [[ "$KEEP_TEMP" == "1" ]]; then
      warn "Clone temporaire conservé: $TMP_ROOT"
    else
      rm -rf "$TMP_ROOT" || true
    fi
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

detect_platform() {
  local us um
  us="$(uname -s 2>/dev/null || true)"
  um="$(uname -m 2>/dev/null || true)"

  case "$us" in
    Darwin) OS="macos" ;;
    Linux) OS="linux" ;;
    MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
    *) fail "Système non supporté: ${us:-unknown}" ;;
  esac

  case "$um" in
    x86_64|amd64) ARCH="x64" ;;
    arm64|aarch64) ARCH="arm64" ;;
    *) fail "Architecture non supportée: ${um:-unknown}" ;;
  esac

  case "$OS/$ARCH" in
    linux/x64)   TARGET="linux-x64";   DIST_BRANCH="distribution/linux-x64" ;;
    linux/arm64) TARGET="linux-arm64"; DIST_BRANCH="distribution/linux-arm64" ;;
    macos/arm64) TARGET="macos-arm64"; DIST_BRANCH="distribution/macos-arm64" ;;
    macos/x64)   TARGET="macos-x64";   DIST_BRANCH="distribution/macos-x64" ;;
    windows/x64) TARGET="windows-x64"; DIST_BRANCH="distribution/windows-x64" ;;
    windows/arm64) fail "Windows ARM64 n’est pas publié actuellement." ;;
    *) fail "Target non supportée: $OS/$ARCH" ;;
  esac
}

remote_url() {
  printf 'https://github.com/%s.git\n' "$REPO"
}

manifest_value() {
  local text="$1" key="$2"
  printf '%s\n' "$text" | sed -n "s/^${key}=//p" | head -n1
}

verify_channel() {
  local line
  line="$(git ls-remote --heads "$(remote_url)" "refs/heads/$DIST_BRANCH" 2>/dev/null || true)"
  [[ -n "$line" ]] || fail "Aucun payload vert publié pour $TARGET."
  ok "Canal disponible: $DIST_BRANCH @ ${line%%[[:space:]]*}"
}

clone_bootstrap() {
  TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/fileflow-install.XXXXXXXX")"
  SRC_DIR="$TMP_ROOT/FileFlow"

  log "Téléchargement du bootstrap FileFlow ($REF)…"
  git clone --quiet --depth 1 --single-branch --branch "$REF" "$(remote_url)" "$SRC_DIR"
  [[ -d "$SRC_DIR/.git" ]] || fail "Clone FileFlow impossible."
}

inspect_payload() {
  PAYLOAD_REF="refs/fileflow/bootstrap/$TARGET"
  git -C "$SRC_DIR" fetch --quiet --depth=1 origin "refs/heads/$DIST_BRANCH:$PAYLOAD_REF"

  local manifest version source_sha package_name package_sha channel
  manifest="$(git -C "$SRC_DIR" show "$PAYLOAD_REF:manifest.env")" || fail "manifest.env absent."
  version="$(manifest_value "$manifest" VERSION)"
  source_sha="$(manifest_value "$manifest" SOURCE_SHA)"
  package_name="$(manifest_value "$manifest" PACKAGE_NAME)"
  package_sha="$(manifest_value "$manifest" PACKAGE_SHA256)"
  channel="$(manifest_value "$manifest" CHANNEL)"

  [[ -n "$version" && -n "$source_sha" && -n "$package_name" && -n "$package_sha" ]] \
    || fail "Manifest incomplet."

  printf '\n'
  log "Version      : $version"
  log "Source SHA   : $source_sha"
  log "Target       : $TARGET"
  log "Canal        : ${channel:-$DIST_BRANCH}"
  log "Package      : $package_name"
  log "SHA-256      : $package_sha"
  printf '\n'
}

windows_powershell() {
  command -v powershell.exe || command -v pwsh.exe || command -v powershell || command -v pwsh
}

repair_windows_stale_marker() {
  [[ "$OS" == "windows" ]] || return 0

  local ps psfile pswin
  ps="$(windows_powershell)"
  psfile="$TMP_ROOT/fileflow-repair-marker.ps1"

  cat > "$psfile" <<'POWERSHELL'
$marker = Join-Path $env:LOCALAPPDATA 'FileFlow\install.env'

$exeCandidates = @(
  (Join-Path $env:LOCALAPPDATA 'Programs\FileFlow\FileFlow.exe'),
  (Join-Path $env:LOCALAPPDATA 'FileFlow\FileFlow.exe'),
  (Join-Path $env:ProgramFiles 'FileFlow\FileFlow.exe')
) | Where-Object { $_ }

$installedExe = $exeCandidates |
  Where-Object { Test-Path -LiteralPath $_ } |
  Select-Object -First 1

$uninstall = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
) | ForEach-Object {
  Get-ItemProperty $_ -ErrorAction SilentlyContinue
} | Where-Object {
  $_.DisplayName -and $_.DisplayName -match '^FileFlow(?:\s|$)'
} | Select-Object -First 1

if ((Test-Path -LiteralPath $marker) -and -not $installedExe -and -not $uninstall) {
  Remove-Item -LiteralPath $marker -Force
  Write-Host '[FileFlow] Marqueur install.env orphelin supprimé.'
}
POWERSHELL

  if command -v cygpath >/dev/null 2>&1; then
    pswin="$(cygpath -w "$psfile")"
  else
    pswin="$psfile"
  fi

  "$ps" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$pswin"
}

run_official_installer() {
  case "$OS" in
    linux|macos)
      [[ -f "$SRC_DIR/install.sh" ]] || fail "install.sh absent de main."
      chmod +x "$SRC_DIR/install.sh"
      (
        cd "$SRC_DIR"
        bash ./install.sh --force
      )
      ;;
    windows)
      local ps installer
      ps="$(windows_powershell)"
      [[ -f "$SRC_DIR/install.ps1" ]] || fail "install.ps1 absent de main."
      if command -v cygpath >/dev/null 2>&1; then
        installer="$(cygpath -w "$SRC_DIR/install.ps1")"
      else
        installer="$SRC_DIR/install.ps1"
      fi
      "$ps" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$installer" -Force
      ;;
  esac
}

verify_installation() {
  case "$OS" in
    linux)
      [[ -x "$HOME/.local/opt/fileflow/FileFlow.AppImage" || -x "$HOME/.local/bin/fileflow" ]] \
        || fail "Installation Linux non détectée."
      ;;
    macos)
      [[ -d "/Applications/FileFlow.app" || -d "$HOME/Applications/FileFlow.app" ]] \
        || fail "FileFlow.app non détecté."
      ;;
    windows)
      local ps
      ps="$(windows_powershell)"
      "$ps" -NoLogo -NoProfile -ExecutionPolicy Bypass -Command '
$paths = @(
  (Join-Path $env:LOCALAPPDATA "Programs\FileFlow\FileFlow.exe"),
  (Join-Path $env:LOCALAPPDATA "FileFlow\FileFlow.exe"),
  (Join-Path $env:ProgramFiles "FileFlow\FileFlow.exe")
) | Where-Object { $_ }

if ($paths | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1) {
  exit 0
}

$entry = @(
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
  "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*"
) | ForEach-Object {
  Get-ItemProperty $_ -ErrorAction SilentlyContinue
} | Where-Object {
  $_.DisplayName -and $_.DisplayName -match "^FileFlow(?:\s|$)"
} | Select-Object -First 1

if ($entry) { exit 0 }
exit 1
' || fail "Installation Windows non détectée."
      ;;
  esac

  ok "FileFlow est installé."
}

main() {
  command -v git >/dev/null 2>&1 || fail "Git est requis."

  detect_platform

  if [[ "$OS" == "windows" ]]; then
    windows_powershell >/dev/null || fail "PowerShell est requis sous Windows."
  fi

  log "Installation générale FileFlow"
  log "Dépôt       : $REPO"
  log "Source      : $REF"
  log "Plateforme  : $OS/$ARCH"
  log "Target      : $TARGET"

  verify_channel
  clone_bootstrap
  inspect_payload
  repair_windows_stale_marker
  run_official_installer
  verify_installation

  ok "Installation générale FileFlow terminée."
}

main "$@"
