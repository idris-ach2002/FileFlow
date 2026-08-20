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

# ============================================================
# Dépendances locales
# ============================================================

command -v git >/dev/null 2>&1 ||
  die "git est requis"

command -v gh >/dev/null 2>&1 ||
  die "GitHub CLI (gh) est requis"

gh auth status >/dev/null 2>&1 ||
  die "GitHub CLI non authentifié. Lance: gh auth login"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
  die "Ce script doit être exécuté dans le dépôt FileFlow"

if [ -n "$(git status --porcelain)" ]; then
  die "Le dépôt contient des modifications locales. Commit/stash requis."
fi

# ============================================================
# Synchronisation Git
# ============================================================

BRANCH="$(git branch --show-current)"

[ -n "$BRANCH" ] ||
  die "HEAD détaché non supporté par le launcher"

info "Synchronisation Git : $BRANCH"

git pull --ff-only

HEAD_SHA="$(git rev-parse HEAD)"
SHORT_SHA="$(git rev-parse --short HEAD)"

REPO="$(
  gh repo view \
    --json nameWithOwner \
    --jq '.nameWithOwner'
)"

ok "Repository : $REPO"
ok "Commit     : $SHORT_SHA"

# ============================================================
# Détection OS / architecture
# ============================================================

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
    die "Windows sera activé dès que Native Windows sera certifié."
    ;;

  *)
    die "Système non supporté: $OS"
    ;;
esac

echo
echo "============================================================"
echo "FileFlow"
echo "============================================================"
echo "OS       : $OS"
echo "Arch     : $ARCH"
echo "Target   : $TARGET"
echo "Workflow : $WORKFLOW"
echo "Artifact : $ARTIFACT"
echo "Commit   : $SHORT_SHA"
echo "============================================================"

# ============================================================
# Vérifier si un run contient réellement notre artefact
# ============================================================

artifact_exists() {
  local run_id="$1"
  local found

  found="$(
    gh api \
      "repos/$REPO/actions/runs/$run_id/artifacts" \
      --jq ".artifacts[]
        | select(.name == \"$ARTIFACT\" and .expired == false)
        | .id" \
      2>/dev/null \
      | head -n 1 \
      || true
  )"

  [ -n "$found" ]
}

# ============================================================
# Chercher un SUCCESS pour exactement HEAD
# ============================================================

find_success_artifact_run() {
  local run_id

  while IFS= read -r run_id; do
    [ -n "$run_id" ] || continue

    if artifact_exists "$run_id"; then
      printf '%s\n' "$run_id"
      return 0
    fi
  done < <(
    gh run list \
      --workflow "$WORKFLOW" \
      --commit "$HEAD_SHA" \
      --status success \
      --limit 30 \
      --json databaseId \
      --jq '.[].databaseId'
  )

  return 1
}

# ============================================================
# Chercher un workflow_dispatch déjà actif
# ============================================================

find_active_run() {
  gh run list \
    --workflow "$WORKFLOW" \
    --commit "$HEAD_SHA" \
    --event workflow_dispatch \
    --limit 30 \
    --json databaseId,status,createdAt \
    --jq '
      [
        .[]
        | select(
            .status == "queued"
            or .status == "in_progress"
            or .status == "waiting"
            or .status == "requested"
            or .status == "pending"
          )
      ]
      | sort_by(.createdAt)
      | reverse
      | .[0].databaseId // empty
    '
}

latest_dispatch_run() {
  gh run list \
    --workflow "$WORKFLOW" \
    --commit "$HEAD_SHA" \
    --event workflow_dispatch \
    --limit 30 \
    --json databaseId,createdAt \
    --jq '
      sort_by(.createdAt)
      | reverse
      | .[0].databaseId // empty
    '
}

# ============================================================
# Résolution du run
# ============================================================

RUN_ID="$(find_success_artifact_run || true)"

if [ -n "$RUN_ID" ]; then
  ok "Artefact existant trouvé dans le run $RUN_ID"
else
  RUN_ID="$(find_active_run || true)"

  if [ -n "$RUN_ID" ]; then
    info "Workflow déjà actif pour $SHORT_SHA : $RUN_ID"
  else
    info "Aucun artefact $TARGET pour $SHORT_SHA"
    info "Déclenchement de $WORKFLOW uniquement"

    BEFORE_ID="$(latest_dispatch_run || true)"

    gh workflow run "$WORKFLOW" --ref "$BRANCH"

    info "Recherche du run créé..."

    RUN_ID=""

    for _ in $(seq 1 30); do
      CANDIDATE="$(latest_dispatch_run || true)"

      if [ -n "$CANDIDATE" ] && [ "$CANDIDATE" != "$BEFORE_ID" ]; then
        RUN_ID="$CANDIDATE"
        break
      fi

      sleep 2
    done

    [ -n "$RUN_ID" ] ||
      die "Impossible de retrouver le workflow déclenché"
  fi

  info "GitHub Actions run : $RUN_ID"

  gh run watch "$RUN_ID" --exit-status ||
    die "Le workflow $WORKFLOW a échoué"

  # GitHub peut avoir une petite latence entre la fin du run
  # et la disponibilité de l'artefact via l'API.
  info "Vérification de l'artefact $ARTIFACT"

  FOUND=""

  for _ in $(seq 1 20); do
    RUN_WITH_ARTIFACT="$(find_success_artifact_run || true)"

    if [ -n "$RUN_WITH_ARTIFACT" ]; then
      FOUND="$RUN_WITH_ARTIFACT"
      break
    fi

    sleep 2
  done

  [ -n "$FOUND" ] ||
    die "Workflow vert mais artefact '$ARTIFACT' introuvable"

  RUN_ID="$FOUND"
fi

ok "Run sélectionné : $RUN_ID"

# ============================================================
# Téléchargement
# ============================================================

CACHE="$ROOT/.fileflow"
DEST="$CACHE/artifacts/$HEAD_SHA/$TARGET"

rm -rf "$DEST"
mkdir -p "$DEST"

info "Téléchargement de $ARTIFACT"

gh run download "$RUN_ID" \
  --name "$ARTIFACT" \
  --dir "$DEST"

ok "Artefact téléchargé"

echo
find "$DEST" -maxdepth 10 -type f -print

# ============================================================
# macOS
# ============================================================

if [ "$OS" = "Darwin" ]; then
  DMG="$(
    find "$DEST" \
      -type f \
      -iname '*.dmg' \
      -print \
      -quit
  )"

  [ -n "$DMG" ] ||
    die "Aucun DMG trouvé"

  ok "DMG : $(basename "$DMG")"

  MOUNT="$CACHE/mount-$TARGET"
  RUNTIME="$CACHE/runtime/$TARGET"

  rm -rf "$MOUNT" "$RUNTIME"
  mkdir -p "$MOUNT" "$RUNTIME"

  cleanup_mount() {
    hdiutil detach "$MOUNT" >/dev/null 2>&1 || true
  }

  trap cleanup_mount EXIT INT TERM

  info "Montage du DMG"

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
    die "Application .app introuvable dans le DMG"

  mkdir -p "$RUNTIME"

  info "Installation dans le runtime FileFlow"

  ditto "$APP" "$RUNTIME/FileFlow.app"

  cleanup_mount
  trap - EXIT INT TERM

  info "Lancement de FileFlow"

  open "$RUNTIME/FileFlow.app"

  echo
  echo "============================================================"
  echo "FileFlow lancé"
  echo "============================================================"
  echo "$RUNTIME/FileFlow.app"

  exit 0
fi

# ============================================================
# Linux
# ============================================================

if [ "$OS" = "Linux" ]; then
  APPIMAGE="$(
    find "$DEST" \
      -type f \
      -iname '*.AppImage' \
      -print \
      -quit
  )"

  [ -n "$APPIMAGE" ] ||
    die "Aucun AppImage trouvé"

  chmod +x "$APPIMAGE"

  info "Lancement de FileFlow"

  exec "$APPIMAGE" "$@"
fi
