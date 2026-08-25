#!/bin/sh
set -eu

base_url=${FILEFLOW_DOWNLOAD_PORTAL:-https://fileflow-downloads.pages.dev}
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) platform=darwin-aarch64 ;;
  Darwin-x86_64) platform=darwin-x86_64 ;;
  Linux-aarch64|Linux-arm64) platform=linux-aarch64 ;;
  Linux-x86_64|Linux-amd64) platform=linux-x86_64 ;;
  *) echo "FileFlow Setup ne prend pas encore en charge cette plateforme." >&2; exit 2 ;;
esac

temporary=$(mktemp -d "${TMPDIR:-/tmp}/fileflow-setup.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
manifest="$temporary/downloads.json"
curl -fLsS --connect-timeout 15 --max-time 60 --retry 3 --retry-all-errors "$base_url/api/downloads" -o "$manifest"

field() {
  awk -v platform="$platform" -v field="$1" '
    $0 ~ "\"" platform "\"" { inside=1; next }
    inside && /"setup"[[:space:]]*:/ { setup=1; next }
    setup && $0 ~ "\"" field "\"[[:space:]]*:" {
      value=$0; sub(/^[^:]*:[[:space:]]*\"/, "", value); sub(/\"[,]?[[:space:]]*$/, "", value); print value; exit
    }
  ' "$manifest"
}

url=$(field url)
sha256=$(field sha256)
name=$(field name)
test -n "$url" && test -n "$sha256" && test -n "$name" || { echo "Manifeste FileFlow invalide." >&2; exit 3; }
case "$url" in https://github.com/idris-ach2002/FileFlow/releases/download/*) ;; *) echo "URL FileFlow non autorisée." >&2; exit 3 ;; esac
case "$name" in ''|.|..|*/*|*\\*) echo "Nom de paquet FileFlow invalide." >&2; exit 3 ;; esac
case "$sha256" in *[!0-9A-Fa-f]*|'') echo "SHA-256 FileFlow invalide." >&2; exit 3 ;; esac
test "${#sha256}" -eq 64 || { echo "SHA-256 FileFlow invalide." >&2; exit 3; }
installer="$temporary/$name"
echo "Téléchargement de FileFlow Setup pour $platform…"
curl -fL --progress-bar --connect-timeout 15 --max-time 900 --retry 3 --retry-all-errors "$url" -o "$installer"
if command -v shasum >/dev/null 2>&1; then actual=$(shasum -a 256 "$installer" | awk '{print $1}'); else actual=$(sha256sum "$installer" | awk '{print $1}'); fi
test "$actual" = "$sha256" || { echo "Échec SHA-256 : téléchargement refusé." >&2; exit 4; }
echo "✓ SHA-256 vérifié"

case "$platform" in
  darwin-*) open "$installer"; echo "Le disque FileFlow Setup est ouvert. Double-cliquez sur FileFlowSetup." ;;
  linux-*) chmod +x "$installer"; "$installer" ;;
esac
