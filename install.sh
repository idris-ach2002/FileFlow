#!/usr/bin/env bash
set -Eeuo pipefail

# FileFlow production bootstrap installer.
# No Node / pnpm / Rust / Python / GitHub CLI required.
#
# Modes:
#   ./install.sh
#   ./install.sh --mode dev
#   ./install.sh --version 1.0.1
#   ./install.sh --no-launch
#   ./install.sh --linux-user
#
# Environment:
#   FILEFLOW_INSTALL_MODE=user|dev
#   FILEFLOW_VERSION=1.0.1

REPO="idris-ach2002/FileFlow"
MODE="${FILEFLOW_INSTALL_MODE:-user}"
VERSION="${FILEFLOW_VERSION:-}"
NO_LAUNCH=0
ALLOW_UNSIGNED=0
LINUX_USER=0
STEP="initialisation"
ASSET=""
TAG=""
DOWNLOAD_URL=""

SCRIPT_DIR="$(
  cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null &&
  pwd
)" || SCRIPT_DIR="$PWD"

usage() {
  cat <<'EOF'
Installation FileFlow

Usage:
  ./install.sh
  ./install.sh --mode user
  ./install.sh --mode dev
  ./install.sh --version 1.0.1
  ./install.sh --no-launch
  ./install.sh --linux-user

Options:
  --mode user       messages simples destinés à l'utilisateur (défaut)
  --mode dev        diagnostics détaillés + log technique
  --version X.Y.Z   installe une version précise
  --no-launch       installe sans lancer FileFlow
  --linux-user      force une installation Linux locale AppImage sans sudo
  --allow-unsigned  DEV uniquement: autorise un bundle macOS non approuvé
  -h, --help        aide
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode)
      [ "$#" -ge 2 ] || { echo "Valeur manquante pour --mode" >&2; exit 2; }
      MODE="$2"
      shift 2
      ;;
    --version)
      [ "$#" -ge 2 ] || { echo "Valeur manquante pour --version" >&2; exit 2; }
      VERSION="$2"
      shift 2
      ;;
    --no-launch)
      NO_LAUNCH=1
      shift
      ;;
    --linux-user)
      LINUX_USER=1
      shift
      ;;
    --allow-unsigned)
      ALLOW_UNSIGNED=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Option inconnue: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$MODE" in
  user|dev) ;;
  *)
    echo "Mode invalide '$MODE'. Utilise user ou dev." >&2
    exit 2
    ;;
esac

if [ "$ALLOW_UNSIGNED" -eq 1 ] && [ "$MODE" != "dev" ]; then
  echo "--allow-unsigned est réservé au mode dev." >&2
  exit 2
fi

OS="$(uname -s 2>/dev/null || echo unknown)"
ARCH="$(uname -m 2>/dev/null || echo unknown)"

if [ "$OS" = "Darwin" ]; then
  LOG_DIR="$HOME/Library/Logs/FileFlow"
else
  LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/fileflow"
fi

mkdir -p "$LOG_DIR" 2>/dev/null || true
LOG_FILE="$LOG_DIR/install-$(date '+%Y%m%d-%H%M%S').log"

log() {
  printf '%s [%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$STEP" "$*" >>"$LOG_FILE" 2>/dev/null || true
}

say() {
  printf '%s\n' "$*"
  log "$*"
}

dev() {
  if [ "$MODE" = "dev" ]; then
    printf '[DEV] %s\n' "$*"
  fi
  log "[DEV] $*"
}

fail() {
  local code="$1"
  local user_message="$2"
  local developer_message="${3:-$2}"

  printf '\nFileFlow n’a pas pu terminer l’installation.\n'
  printf 'Code : %s\n' "$code"
  printf '%s\n' "$user_message"

  if [ "$MODE" = "dev" ]; then
    printf '\n--- Diagnostic développeur ---\n'
    printf 'Étape       : %s\n' "$STEP"
    printf 'OS/arch     : %s / %s\n' "$OS" "$ARCH"
    printf 'Version     : %s\n' "${VERSION:-inconnue}"
    printf 'Tag         : %s\n' "${TAG:-non résolu}"
    printf 'Asset       : %s\n' "${ASSET:-non résolu}"
    printf 'URL         : %s\n' "${DOWNLOAD_URL:-non résolue}"
    printf 'Détail      : %s\n' "$developer_message"
    printf 'Log         : %s\n' "$LOG_FILE"
  else
    printf 'Relance avec "--mode dev" si un diagnostic technique est nécessaire.\n'
  fi

  log "FAIL $code: $developer_message"
  exit 1
}

on_unexpected_error() {
  local exit_code="$1"
  local line="$2"
  local command="$3"

  trap - ERR

  fail \
    "FF-I-999" \
    "Une erreur système inattendue est survenue. Réessaie. Si le problème persiste, transmets le code FF-I-999." \
    "exit=$exit_code line=$line command=$command"
}

trap 'on_unexpected_error "$?" "$LINENO" "$BASH_COMMAND"' ERR

STEP="préparation"
dev "log=$LOG_FILE"
dev "script_dir=$SCRIPT_DIR"
dev "os=$OS arch=$ARCH"

TMP_ROOT="$(mktemp -d 2>/dev/null || mktemp -d -t fileflow)"
cleanup() {
  if [ -n "${MOUNT_POINT:-}" ] && [ -d "${MOUNT_POINT:-}" ]; then
    hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMP_ROOT" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

http_get() {
  local url="$1"
  local output="$2"

  dev "GET $url -> $output"

  if command -v curl >/dev/null 2>&1; then
    curl \
      --fail \
      --location \
      --silent \
      --show-error \
      --connect-timeout 15 \
      --retry 3 \
      --retry-delay 2 \
      --output "$output" \
      "$url"
    return
  fi

  if command -v wget >/dev/null 2>&1; then
    wget \
      --quiet \
      --timeout=20 \
      --tries=3 \
      --output-document="$output" \
      "$url"
    return
  fi

  fail \
    "FF-I-010" \
    "Aucun outil de téléchargement n’est disponible. Installe curl ou wget puis relance l’installation." \
    "neither curl nor wget is installed"
}

resolve_version() {
  STEP="résolution de la version"

  if [ -n "$VERSION" ]; then
    return
  fi

  local local_config="$SCRIPT_DIR/src-tauri/tauri.conf.json"

  if [ -f "$local_config" ]; then
    VERSION="$(
      sed -nE 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' \
        "$local_config" |
        head -n 1
    )"
  fi

  if [ -n "$VERSION" ]; then
    dev "version from local tauri.conf.json: $VERSION"
    return
  fi

  local remote_config="$TMP_ROOT/tauri.conf.json"

  if ! http_get \
    "https://raw.githubusercontent.com/$REPO/main/src-tauri/tauri.conf.json" \
    "$remote_config"; then
    fail \
      "FF-I-002" \
      "Impossible de contacter le serveur de téléchargement FileFlow. Vérifie la connexion Internet puis réessaie." \
      "failed to download main/src-tauri/tauri.conf.json"
  fi

  VERSION="$(
    sed -nE 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' \
      "$remote_config" |
      head -n 1
  )"

  [ -n "$VERSION" ] ||
    fail \
      "FF-I-011" \
      "La version FileFlow disponible n’a pas pu être déterminée." \
      "version key missing in remote tauri.conf.json"
}

validate_version() {
  case "$VERSION" in
    ''|*[!0-9.]*)
      fail \
        "FF-I-011" \
        "La version FileFlow demandée est invalide." \
        "invalid version string: $VERSION"
      ;;
  esac

  if ! printf '%s' "$VERSION" |
      grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    fail \
      "FF-I-011" \
      "La version FileFlow demandée est invalide." \
      "version does not match X.Y.Z: $VERSION"
  fi
}

sha256_file() {
  local file="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
    return
  fi

  fail \
    "FF-I-010" \
    "La vérification de sécurité SHA-256 n’est pas disponible sur cette machine." \
    "neither sha256sum nor shasum is available"
}

verify_checksum() {
  local file="$1"
  local checksum_file="$2"
  local name
  local expected
  local actual

  STEP="vérification d’intégrité"

  name="$(basename "$file")"

  expected="$(
    awk -v name="$name" '
      {
        path=$2
        sub(/^\*/, "", path)
        n=split(path, parts, "/")
        if (parts[n] == name) {
          print $1
          exit
        }
      }
    ' "$checksum_file"
  )"

  [ -n "$expected" ] ||
    fail \
      "FF-I-004" \
      "Le fichier de contrôle de cette version ne contient pas le paquet attendu. L’installation a été arrêtée par sécurité." \
      "checksum entry not found for $name"

  actual="$(sha256_file "$file")"

  [ "$expected" = "$actual" ] ||
    fail \
      "FF-I-004" \
      "Le paquet téléchargé ne correspond pas à la signature d’intégrité publiée. Il ne sera pas installé." \
      "checksum mismatch expected=$expected actual=$actual file=$name"

  dev "SHA256 verified: $actual"
}

download_release_asset() {
  local asset="$1"
  local destination="$2"

  DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/$asset"

  if ! http_get "$DOWNLOAD_URL" "$destination"; then
    fail \
      "FF-I-003" \
      "Cette version de FileFlow n’est pas encore publiée pour cette plateforme, ou le téléchargement est momentanément indisponible." \
      "release asset download failed: tag=$TAG asset=$asset"
  fi
}

download_checksum() {
  local checksum_name="$1"
  local destination="$2"
  local url="https://github.com/$REPO/releases/download/$TAG/$checksum_name"

  if ! http_get "$url" "$destination"; then
    fail \
      "FF-I-004" \
      "Le fichier de contrôle SHA-256 de la release est introuvable. L’installation a été arrêtée par sécurité." \
      "checksum file download failed: $url"
  fi
}

resolve_version
validate_version

say ""
say "FileFlow $VERSION"
say "Plateforme détectée : $OS / $ARCH"
dev "repository=$REPO"

case "$OS" in
  Darwin)
    STEP="détection macOS"

    case "$ARCH" in
      arm64|aarch64)
        ASSET="FileFlow-macOS-arm64.dmg"
        ;;
      x86_64|amd64)
        ASSET="FileFlow-macOS-x64.dmg"
        ;;
      *)
        fail \
          "FF-I-001" \
          "Cette architecture Mac n’est pas prise en charge par FileFlow." \
          "unsupported macOS architecture: $ARCH"
        ;;
    esac

    TAG="macos-v$VERSION"
    CHECKSUM_NAME="SHA256SUMS-macos"
    ;;

  Linux)
    STEP="détection Linux"

    case "$ARCH" in
      x86_64|amd64)
        RELEASE_ARCH="x64"
        ;;
      arm64|aarch64)
        RELEASE_ARCH="arm64"
        ;;
      *)
        fail \
          "FF-I-001" \
          "Cette architecture Linux n’est pas prise en charge par FileFlow." \
          "unsupported Linux architecture: $ARCH"
        ;;
    esac

    TAG="linux-v$VERSION"
    CHECKSUM_NAME="SHA256SUMS-linux"
    ;;

  *)
    fail \
      "FF-I-001" \
      "Ce script est destiné à macOS et Linux. Sur Windows, utilise install.ps1." \
      "unsupported Unix bootstrap platform: $OS"
    ;;
esac

CHECKSUM_FILE="$TMP_ROOT/$CHECKSUM_NAME"
download_checksum "$CHECKSUM_NAME" "$CHECKSUM_FILE"

if [ "$OS" = "Darwin" ]; then
  STEP="téléchargement macOS"

  PACKAGE="$TMP_ROOT/$ASSET"
  download_release_asset "$ASSET" "$PACKAGE"
  verify_checksum "$PACKAGE" "$CHECKSUM_FILE"

  STEP="montage du DMG"
  MOUNT_POINT="$TMP_ROOT/mount"
  mkdir -p "$MOUNT_POINT"

  if ! hdiutil attach \
    "$PACKAGE" \
    -nobrowse \
    -readonly \
    -mountpoint "$MOUNT_POINT" \
    >/dev/null; then
    fail \
      "FF-I-007" \
      "Le disque d’installation FileFlow n’a pas pu être ouvert." \
      "hdiutil attach failed for $PACKAGE"
  fi

  SOURCE_APP="$(
    find "$MOUNT_POINT" \
      -maxdepth 3 \
      -type d \
      -name 'FileFlow.app' \
      -print \
      -quit
  )"

  [ -n "$SOURCE_APP" ] ||
    fail \
      "FF-I-007" \
      "Le paquet téléchargé ne contient pas FileFlow.app." \
      "FileFlow.app missing inside DMG"

  STEP="contrôle de confiance macOS"

  if ! codesign --verify --deep --strict "$SOURCE_APP" >/dev/null 2>&1; then
    if [ "$ALLOW_UNSIGNED" -eq 1 ]; then
      dev "WARNING: codesign verification failed but --allow-unsigned is active"
    else
      fail \
        "FF-I-006" \
        "macOS ne reconnaît pas cette copie de FileFlow comme correctement signée. L’installation est bloquée par sécurité." \
        "codesign --verify failed"
    fi
  fi

  if ! spctl --assess --type execute "$SOURCE_APP" >/dev/null 2>&1; then
    if [ "$ALLOW_UNSIGNED" -eq 1 ]; then
      dev "WARNING: Gatekeeper assessment failed but --allow-unsigned is active"
    else
      fail \
        "FF-I-006" \
        "Cette copie de FileFlow n’a pas été approuvée par Gatekeeper. Une release signée/notarisée est requise." \
        "spctl assessment failed"
    fi
  fi

  STEP="installation macOS"

  if pgrep -f '/FileFlow\.app/' >/dev/null 2>&1; then
    fail \
      "FF-I-008" \
      "FileFlow est actuellement ouvert. Ferme l’application puis relance l’installation." \
      "running FileFlow.app process detected"
  fi

  if [ -w "/Applications" ]; then
    INSTALL_BASE="/Applications"
  else
    INSTALL_BASE="$HOME/Applications"
    mkdir -p "$INSTALL_BASE" ||
      fail \
        "FF-I-005" \
        "Le dossier Applications utilisateur n’a pas pu être créé." \
        "cannot create $INSTALL_BASE"
  fi

  DEST_APP="$INSTALL_BASE/FileFlow.app"
  STAGE_APP="$INSTALL_BASE/.FileFlow.app.installing.$$"
  BACKUP_APP="$INSTALL_BASE/.FileFlow.app.backup.$$"

  rm -rf "$STAGE_APP" "$BACKUP_APP"

  if ! ditto "$SOURCE_APP" "$STAGE_APP"; then
    fail \
      "FF-I-008" \
      "FileFlow n’a pas pu être copié dans le dossier Applications." \
      "ditto failed source=$SOURCE_APP destination=$STAGE_APP"
  fi

  if [ -e "$DEST_APP" ]; then
    mv "$DEST_APP" "$BACKUP_APP" ||
      fail \
        "FF-I-005" \
        "L’ancienne installation FileFlow ne peut pas être remplacée. Vérifie les permissions du dossier Applications." \
        "cannot move existing app to backup"
  fi

  if ! mv "$STAGE_APP" "$DEST_APP"; then
    [ -e "$BACKUP_APP" ] && mv "$BACKUP_APP" "$DEST_APP" || true
    fail \
      "FF-I-008" \
      "La nouvelle version n’a pas pu être activée. L’ancienne installation a été restaurée si possible." \
      "atomic activation failed"
  fi

  rm -rf "$BACKUP_APP"

  say ""
  say "✓ FileFlow $VERSION est installé dans :"
  say "  $DEST_APP"
  say "✓ L’application est disponible dans Applications, Launchpad et Spotlight."

  if [ "$NO_LAUNCH" -eq 0 ]; then
    STEP="lancement macOS"

    if ! open "$DEST_APP"; then
      fail \
        "FF-I-009" \
        "FileFlow est installé, mais macOS n’a pas pu le lancer automatiquement. Ouvre-le depuis Applications." \
        "open failed for $DEST_APP"
    fi

    say "✓ FileFlow a été lancé."
  fi

  exit 0
fi

# -----------------------------------------------------------------
# Linux
# -----------------------------------------------------------------

STEP="sélection du paquet Linux"

DISTRO_ID=""
DISTRO_LIKE=""

if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  DISTRO_ID="${ID:-}"
  DISTRO_LIKE="${ID_LIKE:-}"
fi

dev "linux distro id=$DISTRO_ID like=$DISTRO_LIKE"

install_linux_user_appimage() {
  ASSET="FileFlow-Linux-${RELEASE_ARCH}.AppImage"
  PACKAGE="$TMP_ROOT/$ASSET"

  STEP="téléchargement AppImage"
  download_release_asset "$ASSET" "$PACKAGE"
  verify_checksum "$PACKAGE" "$CHECKSUM_FILE"

  chmod +x "$PACKAGE"

  STEP="installation Linux utilisateur"

  APP_DIR="$HOME/.local/opt/fileflow"
  BIN_DIR="$HOME/.local/bin"
  APPS_DIR="$HOME/.local/share/applications"
  ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"

  mkdir -p "$APP_DIR" "$BIN_DIR" "$APPS_DIR" "$ICON_DIR" ||
    fail \
      "FF-I-005" \
      "FileFlow ne peut pas écrire dans les dossiers d’applications de ton utilisateur." \
      "cannot create Linux user install directories"

  DEST_APPIMAGE="$APP_DIR/FileFlow.AppImage"
  STAGE_APPIMAGE="$APP_DIR/.FileFlow.AppImage.installing.$$"

  cp "$PACKAGE" "$STAGE_APPIMAGE" ||
    fail \
      "FF-I-008" \
      "L’AppImage FileFlow n’a pas pu être installée." \
      "copy to $STAGE_APPIMAGE failed"

  chmod +x "$STAGE_APPIMAGE"

  mv -f "$STAGE_APPIMAGE" "$DEST_APPIMAGE" ||
    fail \
      "FF-I-008" \
      "La nouvelle version FileFlow n’a pas pu être activée." \
      "rename to $DEST_APPIMAGE failed"

  WRAPPER="$BIN_DIR/fileflow"

  cat >"$WRAPPER" <<EOF
#!/usr/bin/env bash
set -e
APP="\$HOME/.local/opt/fileflow/FileFlow.AppImage"

if [ ! -x "\$APP" ]; then
  echo "FileFlow: application introuvable: \$APP" >&2
  exit 1
fi

# Le mode extract-and-run évite toute dépendance à FUSE/libfuse2.
APPIMAGE_EXTRACT_AND_RUN=1 exec "\$APP" "\$@"
EOF

  chmod +x "$WRAPPER"

  STEP="intégration au menu Linux"

  ICON_PATH="$ICON_DIR/fileflow.png"
  EXTRACT_DIR="$TMP_ROOT/appimage-extract"
  mkdir -p "$EXTRACT_DIR"

  if (
    cd "$EXTRACT_DIR"
    "$DEST_APPIMAGE" --appimage-extract >/dev/null 2>&1
  ); then
    FOUND_ICON="$(
      find "$EXTRACT_DIR/squashfs-root" \
        -type f \
        \( -iname '*256*.png' -o -iname 'icon.png' -o -iname '*fileflow*.png' \) \
        -print \
        -quit 2>/dev/null || true
    )"

    if [ -n "$FOUND_ICON" ]; then
      cp "$FOUND_ICON" "$ICON_PATH" || true
    fi
  fi

  DESKTOP="$APPS_DIR/fileflow.desktop"

  cat >"$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=FileFlow
GenericName=File conversion utility
Comment=Conversion, compression, organisation et automatisation locale de fichiers
Exec="$WRAPPER"
Icon=$ICON_PATH
Terminal=false
Categories=Utility;
StartupNotify=true
StartupWMClass=FileFlow
EOF

  chmod 0644 "$DESKTOP"

  command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true

  command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -f "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true

  say ""
  say "✓ FileFlow $VERSION est installé pour ton utilisateur."
  say "✓ Il apparaît dans le menu/centre des applications Linux."
  say "✓ Commande terminal : $WRAPPER"

  if [ "$NO_LAUNCH" -eq 0 ]; then
    STEP="lancement Linux"

    if [ "$MODE" = "dev" ]; then
      "$WRAPPER" >>"$LOG_FILE" 2>&1 &
    else
      nohup "$WRAPPER" >/dev/null 2>&1 &
    fi

    say "✓ FileFlow a été lancé."
  fi
}

install_linux_native() {
  local kind="$1"

  if [ "$kind" = "deb" ]; then
    ASSET="FileFlow-Linux-${RELEASE_ARCH}.deb"
  else
    ASSET="FileFlow-Linux-${RELEASE_ARCH}.rpm"
  fi

  PACKAGE="$TMP_ROOT/$ASSET"

  STEP="téléchargement paquet Linux"
  download_release_asset "$ASSET" "$PACKAGE"
  verify_checksum "$PACKAGE" "$CHECKSUM_FILE"

  STEP="installation paquet Linux"

  if ! command -v sudo >/dev/null 2>&1; then
    dev "sudo absent, fallback AppImage user"
    install_linux_user_appimage
    return
  fi

  if [ "$kind" = "deb" ]; then
    if ! sudo apt-get install -y "$PACKAGE"; then
      dev "apt install failed, fallback AppImage user"
      install_linux_user_appimage
      return
    fi
  else
    if command -v dnf >/dev/null 2>&1; then
      if ! sudo dnf install -y "$PACKAGE"; then
        dev "dnf install failed, fallback AppImage user"
        install_linux_user_appimage
        return
      fi
    elif command -v rpm >/dev/null 2>&1; then
      if ! sudo rpm -Uvh "$PACKAGE"; then
        dev "rpm install failed, fallback AppImage user"
        install_linux_user_appimage
        return
      fi
    else
      install_linux_user_appimage
      return
    fi
  fi

  say ""
  say "✓ FileFlow $VERSION est installé comme application système."
  say "✓ Il apparaît dans le menu/centre des applications Linux."

  if [ "$NO_LAUNCH" -eq 0 ]; then
    STEP="lancement Linux"

    DESKTOP_FILE="$(
      find /usr/share/applications "$HOME/.local/share/applications" \
        -maxdepth 1 \
        -type f \
        -iname '*fileflow*.desktop' \
        -print \
        -quit 2>/dev/null || true
    )"

    if [ -n "$DESKTOP_FILE" ] && command -v gtk-launch >/dev/null 2>&1; then
      DESKTOP_ID="$(basename "$DESKTOP_FILE" .desktop)"
      gtk-launch "$DESKTOP_ID" >/dev/null 2>&1 &
      say "✓ FileFlow a été lancé."
      return
    fi

    for candidate in fileflow fileflow-desktop FileFlow; do
      if command -v "$candidate" >/dev/null 2>&1; then
        nohup "$candidate" >/dev/null 2>&1 &
        say "✓ FileFlow a été lancé."
        return
      fi
    done

    say "✓ Installation terminée. Lance FileFlow depuis le menu Applications."
    dev "package installed but executable/desktop launcher discovery failed"
  fi
}

if [ "$LINUX_USER" -eq 1 ]; then
  install_linux_user_appimage
  exit 0
fi

case " $DISTRO_ID $DISTRO_LIKE " in
  *" debian "*|*" ubuntu "*)
    if command -v apt-get >/dev/null 2>&1; then
      install_linux_native deb
    else
      install_linux_user_appimage
    fi
    ;;
  *" fedora "*|*" rhel "*|*" centos "*|*" rocky "*|*" almalinux "*)
    install_linux_native rpm
    ;;
  *)
    install_linux_user_appimage
    ;;
esac
