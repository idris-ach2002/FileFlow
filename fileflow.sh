#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

die() {
  printf '\n[ERREUR] %s\n' "$*" >&2
  exit 1
}

info() {
  printf '\n==> %s\n' "$*"
}

ok() {
  printf '[OK] %s\n' "$*"
}

command -v git >/dev/null 2>&1 ||
  die "git est requis"

command -v gh >/dev/null 2>&1 ||
  die "GitHub CLI (gh) est requis"

gh auth status >/dev/null 2>&1 ||
  die "GitHub CLI non authentifié. Lance: gh auth login"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
  die "Ce script doit être exécuté depuis le dépôt FileFlow"

if [ -n "$(git status --porcelain)" ]; then
  die "Le dépôt contient des modifications locales. Commit/stash avant lancement."
fi

# ------------------------------------------------------------
# Synchronisation Git
# ------------------------------------------------------------

BRANCH="$(git branch --show-current)"

if [ -n "$BRANCH" ]; then
  info "Synchronisation Git : $BRANCH"
  git pull --ff-only
else
  info "HEAD détaché : git pull ignoré"
fi

HEAD_SHA="$(git rev-parse HEAD)"
SHORT_SHA="$(git rev-parse --short HEAD)"

REPO="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"

ok "Repository : $REPO"
ok "Commit     : $SHORT_SHA"

# ------------------------------------------------------------
# Plateforme
# ------------------------------------------------------------

OS="$(uname -s)"
ARCH="$(uname -m)"

WORKFLOW=""
TARGET=""
ARTIFACT=""

case "$OS" in
  Darwin)
    WORKFLOW="native-macos.yml"

    case "$ARCH" in
      arm64|aarch64)
        TARGET="aarch64-apple-darwin"
        ;;
      x86_64|amd64)
        TARGET="x86_64-apple-darwin"
        ;;
      *)
        die "Architecture macOS non supportée: $ARCH"
        ;;
    esac

    ARTIFACT="FileFlow-macos-$TARGET"
    ;;

  Linux)
    WORKFLOW="native-linux.yml"

    case "$ARCH" in
      x86_64|amd64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
      arm64|aarch64)
        TARGET="aarch64-unknown-linux-gnu"
        ;;
      *)
        die "Architecture Linux non supportée: $ARCH"
        ;;
    esac

    ARTIFACT="FileFlow-linux-$TARGET"
    ;;

  MINGW*|MSYS*|CYGWIN*)
    WORKFLOW="native-windows.yml"
    TARGET="x86_64-pc-windows-msvc"
    ARTIFACT="FileFlow-windows-$TARGET"

    die "Support launcher Windows activé lorsque Native Windows sera certifié."
    ;;

  *)
    die "Système non supporté: $OS"
    ;;
esac

echo
echo "============================================================"
echo "FileFlow CI candidate"
echo "============================================================"
echo "OS       : $OS"
echo "Arch     : $ARCH"
echo "Target   : $TARGET"
echo "Workflow : $WORKFLOW"
echo "Artifact : $ARTIFACT"
echo "Commit   : $SHORT_SHA"
echo "============================================================"

# ------------------------------------------------------------
# Chercher un run SUCCESS contenant exactement l'artefact voulu
# pour exactement le commit checkouté.
# ------------------------------------------------------------

find_artifact_run() {
  local run_id

  while IFS= read -r run_id; do
    [ -n "$run_id" ] || continue

    local found

    found="$(
      gh api \
        "repos/$REPO/actions/runs/$run_id/artifacts" \
        --jq \
        --arg artifact "$ARTIFACT" \
        '.artifacts[]
         | select(.name == $artifact and .expired == false)
         | .id' \
        2>/dev/null \
        | head -n1 \
        || true
    )"

    if [ -n "$found" ]; then
      printf '%s\n' "$run_id"
      return 0
    fi
  done < <(
    gh run list \
      --workflow "$WORKFLOW" \
      --commit "$HEAD_SHA" \
      --status success \
      --limit 20 \
      --json databaseId \
      --jq '.[].databaseId'
  )

  return 1
}

RUN_ID="$(find_artifact_run || true)"

# ------------------------------------------------------------
# Aucun artifact pour ce SHA:
# déclencher uniquement la plateforme locale.
# ------------------------------------------------------------

if [ -z "$RUN_ID" ]; then
  info "Aucun artefact $TARGET disponible pour $SHORT_SHA"
  info "Déclenchement de $WORKFLOW uniquement"

  REF="$BRANCH"

  if [ -z "$REF" ]; then
    REF="$HEAD_SHA"
  fi

  gh workflow run "$WORKFLOW" --ref "$REF"

  info "Recherche du nouveau run..."

  RUN_ID=""

  for _ in $(seq 1 30); do
    RUN_ID="$(
      gh run list \
        --workflow "$WORKFLOW" \
        --event workflow_dispatch \
        --limit 30 \
        --json databaseId,headSha,createdAt \
        --jq \
        --arg sha "$HEAD_SHA" \
        '[.[] | select(.headSha == $sha)]
         | sort_by(.createdAt)
         | reverse
         | .[0].databaseId // empty'
    )"

    if [ -n "$RUN_ID" ]; then
      break
    fi

    sleep 2
  done

  [ -n "$RUN_ID" ] ||
    die "Impossible de retrouver le workflow déclenché"

  info "GitHub Actions run: $RUN_ID"

  gh run watch "$RUN_ID" --exit-status ||
    die "Le workflow $WORKFLOW a échoué"

  RUN_ID="$(find_artifact_run || true)"

  [ -n "$RUN_ID" ] ||
    die "Workflow vert mais artefact '$ARTIFACT' introuvable"
fi

ok "Run avec artefact: $RUN_ID"

# ------------------------------------------------------------
# Téléchargement
# ------------------------------------------------------------

CACHE="$ROOT/.fileflow"
DEST="$CACHE/artifacts/$HEAD_SHA/$TARGET"

rm -rf "$DEST"
mkdir -p "$DEST"

info "Téléchargement de $ARTIFACT"

gh run download "$RUN_ID" \
  --name "$ARTIFACT" \
  --dir "$DEST"

ok "Artefact téléchargé"

find "$DEST" -type f -maxdepth 8 -print

# ------------------------------------------------------------
# macOS
# ------------------------------------------------------------

if [ "$OS" = "Darwin" ]; then
  DMG="$(
    find "$DEST" \
      -type f \
      -iname '*.dmg' \
      -print \
      -quit
  )"

  [ -n "$DMG" ] ||
    die "Aucun DMG trouvé dans l'artefact"

  info "DMG: $(basename "$DMG")"

  MOUNT="$CACHE/mount"
  RUNTIME="$CACHE/runtime/$TARGET"

  rm -rf "$MOUNT" "$RUNTIME"
  mkdir -p "$MOUNT" "$RUNTIME"

  cleanup_mount() {
    hdiutil detach "$MOUNT" >/dev/null 2>&1 || true
  }

  trap cleanup_mount EXIT INT TERM

  hdiutil attach \
    "$DMG" \
    -nobrowse \
    -readonly \
    -mountpoint "$MOUNT" \
    >/dev/null

  APP="$(
    find "$MOUNT" \
      -maxdepth 3 \
      -type d \
      -name '*.app' \
      -print \
      -quit
  )"

  [ -n "$APP" ] ||
    die "FileFlow.app introuvable dans le DMG"

  info "Extraction de l'application"

  ditto "$APP" "$RUNTIME/FileFlow.app"

  cleanup_mount
  trap - EXIT INT TERM

  info "Lancement FileFlow"

  open "$RUNTIME/FileFlow.app"

  echo
  echo "FileFlow lancé depuis:"
  echo "$RUNTIME/FileFlow.app"
  exit 0
fi

# ------------------------------------------------------------
# Linux
# ------------------------------------------------------------

if [ "$OS" = "Linux" ]; then
  APPIMAGE="$(
    find "$DEST" \
      -type f \
      -iname '*.AppImage' \
      -print \
      -quit
  )"

  [ -n "$APPIMAGE" ] ||
    die "Aucun AppImage trouvé dans l'artefact"

  chmod +x "$APPIMAGE"

  info "Lancement FileFlow"
  echo "$APPIMAGE"
  echo

  exec "$APPIMAGE"
fi
