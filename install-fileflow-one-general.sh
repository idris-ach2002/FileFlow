#!/usr/bin/env bash
set -Eeuo pipefail

REPOSITORY="${FILEFLOW_REPO:-idris-ach2002/FileFlow}"
ENDPOINT="${FILEFLOW_DOWNLOAD_ENDPOINT:-https://github.com/$REPOSITORY/releases/latest/download/downloads.json}"
KEEP_TEMP="${FILEFLOW_KEEP_TEMP:-0}"
TEMPORARY=""

log(){ printf '\033[1;34m[FileFlow Setup]\033[0m %s\n' "$*"; }
ok(){ printf '\033[1;32m[ OK ]\033[0m %s\n' "$*"; }
fail(){ printf '\033[1;31m[FAIL]\033[0m %s\n' "$*" >&2; exit 1; }
cleanup(){ local status=$?; trap - EXIT INT TERM; [[ -z "$TEMPORARY" || "$KEEP_TEMP" == 1 ]] || rm -rf "$TEMPORARY"; exit "$status"; }
trap cleanup EXIT INT TERM

case "$(uname -s 2>/dev/null)-$(uname -m 2>/dev/null)" in
  Darwin-arm64|Darwin-aarch64) PLATFORM=darwin-aarch64 ;;
  Darwin-x86_64|Darwin-amd64) PLATFORM=darwin-x86_64 ;;
  Linux-aarch64|Linux-arm64) PLATFORM=linux-aarch64 ;;
  Linux-x86_64|Linux-amd64) PLATFORM=linux-x86_64 ;;
  MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) PLATFORM=windows-x86_64 ;;
  *) fail "Cette plateforme n’est pas encore prise en charge." ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl est nécessaire pour télécharger FileFlow Setup."
TEMPORARY="$(mktemp -d "${TMPDIR:-/tmp}/fileflow-setup.XXXXXXXX")"
MANIFEST="$TEMPORARY/downloads.json"
log "Lecture de la dernière release stable et complète…"
curl --fail --location --silent --show-error --connect-timeout 15 --max-time 60 --retry 3 --retry-all-errors "$ENDPOINT" --output "$MANIFEST" || fail "Le service de téléchargement FileFlow est indisponible."

manifest_field(){
  awk -v platform="$PLATFORM" -v field="$1" '
    $0 ~ "\"" platform "\"" { inside=1; next }
    inside && /"setup"[[:space:]]*:/ { setup=1; next }
    setup && $0 ~ "\"" field "\"[[:space:]]*:" {
      value=$0; sub(/^[^:]*:[[:space:]]*\"/, "", value); sub(/\"[,]?[[:space:]]*$/, "", value); print value; exit
    }
  ' "$MANIFEST"
}

URL="$(manifest_field url)"
NAME="$(manifest_field name)"
EXPECTED_SHA="$(manifest_field sha256)"
[[ "$URL" == "https://github.com/$REPOSITORY/releases/download/"* && -n "$NAME" && "$EXPECTED_SHA" =~ ^[0-9a-fA-F]{64}$ ]] || fail "Le manifeste de téléchargement est invalide."
[[ "$NAME" != */* && "$NAME" != *\\* && "$NAME" != "." && "$NAME" != ".." ]] || fail "Le nom du paquet dans le manifeste est invalide."
PACKAGE="$TEMPORARY/$NAME"
log "Téléchargement de $NAME…"
curl --fail --location --progress-bar --connect-timeout 15 --max-time 900 --retry 3 --retry-all-errors "$URL" --output "$PACKAGE" || fail "Le téléchargement de FileFlow Setup a échoué."
if command -v shasum >/dev/null 2>&1; then ACTUAL_SHA="$(shasum -a 256 "$PACKAGE" | awk '{print $1}')"; else ACTUAL_SHA="$(sha256sum "$PACKAGE" | awk '{print $1}')"; fi
ACTUAL_SHA_LOWER="$(printf '%s' "$ACTUAL_SHA" | tr '[:upper:]' '[:lower:]')"
EXPECTED_SHA_LOWER="$(printf '%s' "$EXPECTED_SHA" | tr '[:upper:]' '[:lower:]')"
[[ "$ACTUAL_SHA_LOWER" == "$EXPECTED_SHA_LOWER" ]] || fail "Le SHA-256 ne correspond pas : le fichier est refusé."
ok "Intégrité du téléchargement vérifiée par SHA-256"

case "$PLATFORM" in
  darwin-*) open "$PACKAGE"; ok "Le DMG FileFlow Setup est ouvert." ;;
  linux-*) chmod +x "$PACKAGE"; "$PACKAGE" ;;
  windows-*)
    command -v powershell.exe >/dev/null 2>&1 || fail "PowerShell Windows est introuvable."
    WINDOWS_PACKAGE="$PACKAGE"
    if command -v cygpath >/dev/null 2>&1; then WINDOWS_PACKAGE="$(cygpath -w "$PACKAGE")"; fi
    powershell.exe -NoProfile -NonInteractive -Command "Start-Process -FilePath '$WINDOWS_PACKAGE' -Wait"
    ;;
esac
