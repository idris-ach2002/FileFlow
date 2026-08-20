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

command -v git >/dev/null 2>&1 || die "git est requis"
command -v gh >/dev/null 2>&1 || die "GitHub CLI (gh) est requis"

gh auth status >/dev/null 2>&1 ||
  die "GitHub CLI non authentifié. Lance: gh auth login"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 ||
  die "Ce script doit être lancé depuis le dépôt FileFlow"

if [ -n "$(git status --porcelain)" ]; then
  die "Le dépôt contient des modifications locales. Commit/stash requis."
fi

BRANCH="$(git branch --show-current)"
[ -n "$BRANCH" ] || die "HEAD détaché non supporté"

info "Synchronisation Git : $BRANCH"
git pull --ff-only

HEAD_SHA="$(git rev-parse HEAD)"
SHORT_SHA="$(git rev-parse --short HEAD)"
REPO="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"

ok "Repository : $REPO"
ok "Commit     : $SHORT_SHA"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    WORKFLOW="native-macos.yml"
    PLATFORM="macos"
    case "$ARCH" in
      arm64|aarch64) TARGET="aarch64-apple-darwin" ;;
      x86_64|amd64) TARGET="x86_64-apple-darwin" ;;
      *) die "Architecture macOS non supportée: $ARCH" ;;
    esac
    ;;
  Linux)
    WORKFLOW="native-linux.yml"
    PLATFORM="linux"
    case "$ARCH" in
      x86_64|amd64) TARGET="x86_64-unknown-linux-gnu" ;;
      arm64|aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *) die "Architecture Linux non supportée: $ARCH" ;;
    esac
    ;;
  MINGW*|MSYS*|CYGWIN*)
    die "Launcher Windows volontairement désactivé tant que Native Windows n'est pas certifié."
    ;;
  *)
    die "Système non supporté: $OS"
    ;;
esac

ARTIFACT="fileflow-${PLATFORM}-${TARGET}-${HEAD_SHA}-candidate"

echo
echo "============================================================"
echo "FileFlow candidate"
echo "============================================================"
echo "OS       : $OS"
echo "Arch     : $ARCH"
echo "Target   : $TARGET"
echo "Workflow : $WORKFLOW"
echo "Artifact : $ARTIFACT"
echo "Commit   : $SHORT_SHA"
echo "============================================================"

CACHE="$ROOT/.fileflow"
DEST="$CACHE/artifacts/$HEAD_SHA/$TARGET"
mkdir -p "$DEST"

local_bundle_exists() {
  if [ "$OS" = "Darwin" ]; then
    find "$DEST" -type f -iname '*.dmg' -print -quit 2>/dev/null | grep -q .
  else
    find "$DEST" -type f -iname '*.AppImage' -print -quit 2>/dev/null | grep -q .
  fi
}

find_artifact_run() {
  gh api \
    "repos/$REPO/actions/artifacts?name=$ARTIFACT&per_page=100" \
    --jq '
      [.artifacts[]
       | select(.expired == false)]
      | sort_by(.created_at)
      | reverse
      | .[0].workflow_run.id // empty
    ' 2>/dev/null || true
}

find_active_run() {
  gh run list \
    --workflow "$WORKFLOW" \
    --branch "$BRANCH" \
    --limit 20 \
    --json databaseId,status,createdAt \
    --jq '
      [.[] |
        select(
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

AUTO_EXPECTED=false

if [ "$BRANCH" = "main" ] || [ "$BRANCH" = "develop" ]; then
  AUTO_EXPECTED=true
fi

PR_NUMBER="$(
  gh pr list \
    --head "$BRANCH" \
    --state open \
    --limit 1 \
    --json number,headRefOid \
    --jq ".[0] | select(.headRefOid == \"$HEAD_SHA\") | .number // empty" \
    2>/dev/null || true
)"

if [ -n "$PR_NUMBER" ]; then
  AUTO_EXPECTED=true
  ok "PR ouverte #$PR_NUMBER pour ce SHA"
fi

if local_bundle_exists; then
  ok "Candidate déjà présente localement pour $SHORT_SHA"
else
  RUN_ID="$(find_artifact_run)"

  if [ -n "$RUN_ID" ]; then
    ok "Artefact GitHub déjà disponible dans le run $RUN_ID"
  else
    RUN_ID="$(find_active_run)"

    if [ -n "$RUN_ID" ]; then
      info "Workflow automatique déjà actif : $RUN_ID"
      info "Aucun nouveau workflow ne sera déclenché."

      gh run watch "$RUN_ID" --exit-status ||
        die "Le workflow $WORKFLOW a échoué (run $RUN_ID)"
    else
      if [ "$AUTO_EXPECTED" = true ]; then
        info "GitHub doit lancer automatiquement $WORKFLOW."
        info "Attente du run automatique (aucun workflow manuel ne sera créé)..."

        for _ in $(seq 1 60); do
          RUN_ID="$(find_active_run)"

          if [ -n "$RUN_ID" ]; then
            break
          fi

          RUN_ID="$(find_artifact_run)"
          if [ -n "$RUN_ID" ]; then
            break
          fi

          sleep 2
        done

        [ -n "$RUN_ID" ] ||
          die "Aucun run automatique apparu après 120 s. Je refuse d'en créer un second automatiquement."

        info "Run trouvé : $RUN_ID"

        gh run watch "$RUN_ID" --exit-status ||
          die "Le workflow $WORKFLOW a échoué (run $RUN_ID)"
      else
        info "Aucun run automatique attendu pour cette branche."
        info "Déclenchement manuel unique de $WORKFLOW."

        gh workflow run "$WORKFLOW" --ref "$BRANCH"

        for _ in $(seq 1 60); do
          RUN_ID="$(find_active_run)"
          [ -n "$RUN_ID" ] && break
          sleep 2
        done

        [ -n "$RUN_ID" ] ||
          die "Impossible de retrouver le workflow_dispatch créé"

        gh run watch "$RUN_ID" --exit-status ||
          die "Le workflow $WORKFLOW a échoué (run $RUN_ID)"
      fi
    fi

    info "Attente de l'artefact exact $SHORT_SHA"

    ARTIFACT_RUN=""

    for _ in $(seq 1 45); do
      ARTIFACT_RUN="$(find_artifact_run)"
      [ -n "$ARTIFACT_RUN" ] && break
      sleep 2
    done

    [ -n "$ARTIFACT_RUN" ] ||
      die "Le workflow est terminé mais l'artefact '$ARTIFACT' est absent. Le CI est mal configuré; aucun second build ne sera lancé."

    RUN_ID="$ARTIFACT_RUN"

    rm -rf "$DEST"
    mkdir -p "$DEST"

    info "Téléchargement depuis le run $RUN_ID"

    gh run download "$RUN_ID" \
      --name "$ARTIFACT" \
      --dir "$DEST"

    local_bundle_exists ||
      die "Artefact téléchargé mais bundle attendu introuvable"

    ok "Candidate téléchargée"
  fi

  if ! local_bundle_exists; then
    rm -rf "$DEST"
    mkdir -p "$DEST"

    info "Téléchargement de $ARTIFACT"

    gh run download "$RUN_ID" \
      --name "$ARTIFACT" \
      --dir "$DEST"

    local_bundle_exists ||
      die "Artefact téléchargé mais bundle attendu introuvable"

    ok "Candidate téléchargée"
  fi
fi

if [ "$OS" = "Darwin" ]; then
  DMG="$(find "$DEST" -type f -iname '*.dmg' -print -quit)"
  [ -n "$DMG" ] || die "Aucun DMG trouvé"

  MOUNT="$CACHE/mount-$TARGET"
  RUNTIME="$CACHE/runtime/$HEAD_SHA/$TARGET"

  rm -rf "$MOUNT" "$RUNTIME"
  mkdir -p "$MOUNT" "$RUNTIME"

  cleanup_mount() {
    hdiutil detach "$MOUNT" >/dev/null 2>&1 || true
  }

  trap cleanup_mount EXIT INT TERM

  info "Montage de $(basename "$DMG")"

  hdiutil attach \
    "$DMG" \
    -nobrowse \
    -readonly \
    -mountpoint "$MOUNT" \
    >/dev/null

  APP="$(find "$MOUNT" -maxdepth 3 -type d -name '*.app' -print -quit)"
  [ -n "$APP" ] || die "Application .app introuvable dans le DMG"

  ditto "$APP" "$RUNTIME/FileFlow.app"

  cleanup_mount
  trap - EXIT INT TERM

  info "Lancement FileFlow $SHORT_SHA"
  open "$RUNTIME/FileFlow.app"

  ok "FileFlow lancé depuis $RUNTIME/FileFlow.app"
  exit 0
fi

APPIMAGE="$(find "$DEST" -type f -iname '*.AppImage' -print -quit)"
[ -n "$APPIMAGE" ] || die "Aucun AppImage trouvé"

chmod +x "$APPIMAGE"

info "Lancement FileFlow $SHORT_SHA"
exec "$APPIMAGE" "$@"
