#!/usr/bin/env bash
set -Eeuo pipefail

# FileFlow permanent one-time installer.
# The application binary is built by GitHub Actions. Conversion engines are
# installed on the host once and discovered at runtime from system/user paths.
# After success the source clone is disposable.

MODE="user"
FORCE=0
NO_LAUNCH=0
SKIP_DEPS=0
DOCTOR_ONLY=0
STEP="initialisation"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

usage(){ cat <<'HELP'
FileFlow — installation permanente

Usage:
  ./install.sh
  ./install.sh --mode dev
  ./install.sh --no-launch
  ./install.sh --force
  ./install.sh --skip-deps
  ./install.sh --doctor

Le script installe les moteurs localement sur la machine, puis installe
l'application construite par GitHub Actions. Un dépôt de paquets indisponible
n'arrête pas les autres installations. Après succès, le clone peut être supprimé.
HELP
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:?valeur manquante}"; shift 2;;
    --force) FORCE=1; shift;;
    --no-launch) NO_LAUNCH=1; shift;;
    --skip-deps) SKIP_DEPS=1; shift;;
    --doctor) DOCTOR_ONLY=1; shift;;
    -h|--help) usage; exit 0;;
    *) echo "Option inconnue: $1" >&2; usage >&2; exit 2;;
  esac
done
case "$MODE" in user|dev) ;; *) echo "Mode invalide: $MODE" >&2; exit 2;; esac

OS="$(uname -s 2>/dev/null || echo unknown)"
ARCH="$(uname -m 2>/dev/null || echo unknown)"
case "$OS" in
  MINGW*|MSYS*|CYGWIN*)
    command -v powershell.exe >/dev/null 2>&1 || { echo "PowerShell Windows introuvable." >&2; exit 1; }
    ARGS=(); [ "$MODE" = dev ] && ARGS+=("-Mode" "dev"); [ "$FORCE" -eq 1 ] && ARGS+=("-Force"); [ "$NO_LAUNCH" -eq 1 ] && ARGS+=("-NoLaunch"); [ "$SKIP_DEPS" -eq 1 ] && ARGS+=("-SkipDependencies"); [ "$DOCTOR_ONLY" -eq 1 ] && ARGS+=("-Doctor")
    exec powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$ROOT/install.ps1" "${ARGS[@]}"
    ;;
esac

case "$OS/$ARCH" in
  Linux/x86_64|Linux/amd64) TARGET="linux-x64"; DIST_BRANCH="distribution/linux-x64";;
  Linux/aarch64|Linux/arm64) TARGET="linux-arm64"; DIST_BRANCH="distribution/linux-arm64";;
  Darwin/arm64|Darwin/aarch64) TARGET="macos-arm64"; DIST_BRANCH="distribution/macos-arm64";;
  Darwin/x86_64|Darwin/amd64) TARGET="macos-x64"; DIST_BRANCH="distribution/macos-x64";;
  *) printf 'Plateforme non prise en charge: %s/%s\n' "$OS" "$ARCH" >&2; exit 1;;
esac

if [ "$DOCTOR_ONLY" -eq 1 ]; then
  exec bash "$ROOT/scripts/runtime/doctor.sh"
fi

command -v git >/dev/null 2>&1 || { echo "Git est nécessaire pour récupérer le paquet FileFlow depuis le dépôt cloné." >&2; exit 1; }
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo "Exécute ./install.sh depuis le dépôt FileFlow cloné." >&2; exit 1; }

REMOTE="${FILEFLOW_INSTALL_REMOTE:-origin}"
TMP=""; MOUNT=""; REF=""
if [ "$OS" = Darwin ]; then
  LOG_DIR="$HOME/Library/Logs/FileFlow"; STATE_DIR="$HOME/Library/Application Support/FileFlow"
else
  LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/fileflow"; STATE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/fileflow"
fi
mkdir -p "$LOG_DIR" "$STATE_DIR"
LOG="$LOG_DIR/install-$(date '+%Y%m%d-%H%M%S').log"; MARKER="$STATE_DIR/install.env"
log(){ printf '%s [%s] %s\n' "$(date '+%F %T')" "$STEP" "$*" >>"$LOG" 2>/dev/null || true; }
dev(){ log "[DEV] $*"; [ "$MODE" = dev ] && printf '[DEV] %s\n' "$*" || true; }
fail(){ local c="$1" u="$2" d="${3:-$2}"; printf '\nFileFlow n’a pas pu terminer l’installation.\nCode : %s\n%s\n' "$c" "$u"; if [ "$MODE" = dev ]; then printf '\n--- Diagnostic développeur ---\nÉtape       : %s\nOS/arch     : %s / %s\nDistribution: %s\nRéférence   : %s\nDétail      : %s\nLog         : %s\n' "$STEP" "$OS" "$ARCH" "$DIST_BRANCH" "${REF:-non résolue}" "$d" "$LOG"; else printf 'Relance avec "./install.sh --mode dev" pour le diagnostic technique.\n'; fi; log "FAIL $c: $d"; exit 1; }
cleanup(){ [ -z "${MOUNT:-}" ] || hdiutil detach "$MOUNT" >/dev/null 2>&1 || true; [ -z "${TMP:-}" ] || rm -rf "$TMP" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
trap 's=$?; trap - ERR; fail FF-I-999 "Une erreur système inattendue est survenue." "exit=$s line=$LINENO command=$BASH_COMMAND"' ERR

# 1) Host runtime dependencies. This phase is explicitly best-effort. Every
# engine has independent fallbacks and missing engines do not block app install.
if [ "$SKIP_DEPS" -eq 0 ]; then
  STEP="installation des moteurs locaux"
  printf '\n== 1/3 Moteurs de conversion locaux ==\n'
  if ! bash "$ROOT/scripts/runtime/install-dependencies.sh" 2>&1 | tee -a "$LOG"; then
    dev "dependency helper returned non-zero; continuing by design"
  fi
else
  printf '\n== 1/3 Moteurs de conversion locaux ==\nIgnoré (--skip-deps).\n'
fi

STEP="diagnostic des moteurs"
printf '\n== 2/3 Vérification du runtime ==\n'
bash "$ROOT/scripts/runtime/doctor.sh" 2>&1 | tee -a "$LOG" || true

# 2) Fetch the lightweight application package produced by GitHub Actions.
STEP="récupération du paquet"
printf '\n== 3/3 Installation de FileFlow ==\n'
REF="refs/fileflow/install/$TARGET"; git update-ref -d "$REF" >/dev/null 2>&1 || true
if ! git fetch --quiet --depth=1 "$REMOTE" "refs/heads/$DIST_BRANCH:$REF"; then fail FF-I-003 "Le paquet FileFlow pour $TARGET n’est pas encore publié." "git fetch failed branch=$DIST_BRANCH"; fi

TMP="$(mktemp -d "${TMPDIR:-/tmp}/fileflow-install.XXXXXX")"; MANIFEST="$TMP/manifest.env"
git show "$REF:manifest.env" >"$MANIFEST" || fail FF-I-004 "Le manifeste d’installation FileFlow est absent ou invalide."
manifest(){ sed -n "s/^${1}=//p" "$MANIFEST" | head -n1; }
VERSION="$(manifest VERSION)"; SOURCE_SHA="$(manifest SOURCE_SHA)"; PACKAGE_NAME="$(manifest PACKAGE_NAME)"; PACKAGE_SHA256="$(manifest PACKAGE_SHA256)"; PACKAGE_SIZE="$(manifest PACKAGE_SIZE)"; CHANNEL="$(manifest CHANNEL)"; RUNTIME_MODE="$(manifest RUNTIME_MODE)"
[ -n "$VERSION" ] && [ -n "$PACKAGE_NAME" ] && [ -n "$PACKAGE_SHA256" ] && [ "$RUNTIME_MODE" = system ] || fail FF-I-004 "Le manifeste FileFlow est incomplet ou utilise l’ancien runtime embarqué."

installed_ok(){ [ -f "$MARKER" ] || return 1; if [ "$OS" = Linux ]; then [ -x "$HOME/.local/opt/fileflow/FileFlow.AppImage" ]; else [ -d "/Applications/FileFlow.app" ] || [ -d "$HOME/Applications/FileFlow.app" ]; fi; }
marker_value(){ [ -f "$MARKER" ] || return 0; sed -n "s/^${1}=//p" "$MARKER" | head -n1; }
if [ "$FORCE" -eq 0 ] && installed_ok && [ "$(marker_value PACKAGE_SHA256)" = "$PACKAGE_SHA256" ]; then
  printf '\n✓ FileFlow %s est déjà installé.\nLes moteurs locaux ont été vérifiés/mis à jour.\nLe dépôt cloné peut être supprimé.\n' "$VERSION"
  exit 0
fi

PACKAGE="$TMP/$PACKAGE_NAME"; : >"$PACKAGE"
CHUNKS="$(git ls-tree -r --name-only "$REF" payload/ | LC_ALL=C sort)"; [ -n "$CHUNKS" ] || fail FF-I-004 "Le paquet FileFlow ne contient aucun fragment binaire."
while IFS= read -r chunk; do [ -n "$chunk" ] || continue; dev "assemblage $chunk"; git show "$REF:$chunk" >>"$PACKAGE"; done <<<"$CHUNKS"
ACTUAL_SIZE="$(wc -c <"$PACKAGE" | tr -d ' ')"; [ "$ACTUAL_SIZE" = "$PACKAGE_SIZE" ] || fail FF-I-004 "Le paquet FileFlow est incomplet." "size=$ACTUAL_SIZE expected=$PACKAGE_SIZE"
sha256(){ if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'; else fail FF-I-010 "La vérification SHA-256 n’est pas disponible."; fi; }
ACTUAL_SHA="$(sha256 "$PACKAGE")"; [ "$ACTUAL_SHA" = "$PACKAGE_SHA256" ] || fail FF-I-004 "Le contrôle d’intégrité FileFlow a échoué. Rien n’a été installé." "sha=$ACTUAL_SHA expected=$PACKAGE_SHA256"
dev "version=$VERSION source=$SOURCE_SHA channel=$CHANNEL sha=$ACTUAL_SHA runtime=$RUNTIME_MODE"

if [ "$OS" = Linux ]; then
  STEP="installation Linux"
  APP_DIR="$HOME/.local/opt/fileflow"; BIN_DIR="$HOME/.local/bin"; APPS_DIR="$HOME/.local/share/applications"; ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
  mkdir -p "$APP_DIR" "$BIN_DIR" "$APPS_DIR" "$ICON_DIR"
  APP="$APP_DIR/FileFlow.AppImage"; STAGE="$APP_DIR/.FileFlow.AppImage.installing.$$"; cp "$PACKAGE" "$STAGE"; chmod 0755 "$STAGE"; mv -f "$STAGE" "$APP"
  cat >"$BIN_DIR/fileflow" <<'WRAP'
#!/usr/bin/env bash
set -euo pipefail
APP="$HOME/.local/opt/fileflow/FileFlow.AppImage"
[ -x "$APP" ] || { echo "FileFlow est introuvable: $APP" >&2; exit 1; }
export APPIMAGE_EXTRACT_AND_RUN=1
exec "$APP" "$@"
WRAP
  chmod 0755 "$BIN_DIR/fileflow"
  ICON_SOURCE=""; for c in "$ROOT/src-tauri/icons/128x128@2x.png" "$ROOT/src-tauri/icons/128x128.png" "$ROOT/src-tauri/icons/icon.png"; do [ -f "$c" ] && { ICON_SOURCE="$c"; break; }; done
  if [ -n "$ICON_SOURCE" ]; then cp "$ICON_SOURCE" "$ICON_DIR/fileflow.png"; ICON_VALUE="$ICON_DIR/fileflow.png"; else ICON_VALUE="application-x-executable"; fi
  cat >"$APPS_DIR/fileflow.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Version=1.0
Name=FileFlow
GenericName=Gestionnaire de fichiers
Comment=Conversion, compression et automatisation de fichiers
Exec=$BIN_DIR/fileflow
Icon=$ICON_VALUE
Terminal=false
Categories=Utility;FileTools;
StartupNotify=true
StartupWMClass=FileFlow
DESKTOP
  chmod 0644 "$APPS_DIR/fileflow.desktop"
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true
  APP_LOCATION="$APP"
else
  STEP="installation macOS"; MOUNT="$TMP/mount"; mkdir -p "$MOUNT"; hdiutil attach "$PACKAGE" -nobrowse -readonly -mountpoint "$MOUNT" >/dev/null || fail FF-I-007 "Le disque d’installation FileFlow n’a pas pu être monté."
  SOURCE_APP="$(find "$MOUNT" -maxdepth 3 -type d -name 'FileFlow.app' -print -quit)"; [ -n "$SOURCE_APP" ] || fail FF-I-007 "FileFlow.app est absent du paquet macOS."
  if [ "$CHANNEL" = production ]; then codesign --verify --deep --strict "$SOURCE_APP" >/dev/null 2>&1 || fail FF-I-006 "La signature de FileFlow n’est pas valide."; spctl --assess --type execute "$SOURCE_APP" >/dev/null 2>&1 || fail FF-I-006 "FileFlow n’est pas approuvé par Gatekeeper."; else dev "candidate: notarisation production non imposée"; fi
  if [ -w /Applications ]; then APP_LOCATION="/Applications/FileFlow.app"; else mkdir -p "$HOME/Applications"; APP_LOCATION="$HOME/Applications/FileFlow.app"; fi
  STAGE="${APP_LOCATION}.installing.$$"; BACKUP="${APP_LOCATION}.backup.$$"; rm -rf "$STAGE" "$BACKUP"; ditto "$SOURCE_APP" "$STAGE"; [ ! -e "$APP_LOCATION" ] || mv "$APP_LOCATION" "$BACKUP"
  if ! mv "$STAGE" "$APP_LOCATION"; then [ -e "$BACKUP" ] && mv "$BACKUP" "$APP_LOCATION" || true; fail FF-I-008 "La nouvelle version FileFlow n’a pas pu être activée."; fi
  rm -rf "$BACKUP"; hdiutil detach "$MOUNT" >/dev/null; MOUNT=""
fi

STEP="enregistrement"; cat >"$MARKER" <<MARKER
VERSION=$VERSION
SOURCE_SHA=$SOURCE_SHA
TARGET=$TARGET
CHANNEL=$CHANNEL
PACKAGE_SHA256=$PACKAGE_SHA256
RUNTIME_MODE=system
INSTALLED_AT=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
APP_LOCATION=$APP_LOCATION
MARKER
printf '\n============================================================\n✓ FileFlow %s est installé définitivement\n============================================================\nApplication : %s\nPlateforme  : %s\nRuntime     : dépendances locales système\n\nLe dépôt cloné n’est plus nécessaire et peut être supprimé.\n' "$VERSION" "$APP_LOCATION" "$TARGET"
if [ "$NO_LAUNCH" -eq 0 ]; then STEP="lancement"; if [ "$OS" = Linux ]; then nohup "$HOME/.local/bin/fileflow" >/dev/null 2>&1 & else open "$APP_LOCATION"; fi; printf '✓ FileFlow a été lancé.\n'; fi
