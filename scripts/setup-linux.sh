#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
[ "$(uname -s 2>/dev/null || true)" = Linux ] || { echo 'This helper is for Linux.' >&2; exit 1; }

printf '%s\n' '== FileFlow Linux developer setup =='
if command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update || true
  sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev || {
    echo '[WARN] Some Tauri build prerequisites could not be installed.' >&2
  }
else
  echo '[WARN] Install the Tauri/WebKitGTK development prerequisites for your distribution manually.' >&2
fi
bash "$ROOT/scripts/runtime/install-dependencies.sh"
printf '\nRuntime check:\n'
bash "$ROOT/scripts/runtime/doctor.sh"
printf '\nDeveloper prerequisites prepared. Next: bash scripts/bootstrap.sh\n'
