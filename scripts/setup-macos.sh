#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
[ "$(uname -s 2>/dev/null || true)" = Darwin ] || { echo 'This helper is for macOS.' >&2; exit 1; }

printf '%s\n' '== FileFlow macOS developer setup =='
if ! xcode-select -p >/dev/null 2>&1; then
  echo 'Xcode Command Line Tools are required for development; starting Apple installer.'
  xcode-select --install >/dev/null 2>&1 || true
  echo '[WARN] Finish the Command Line Tools installation before building FileFlow.' >&2
fi
bash "$ROOT/scripts/runtime/install-dependencies.sh"
printf '\nRuntime check:\n'
bash "$ROOT/scripts/runtime/doctor.sh"
printf '\nDeveloper prerequisites prepared. Next: bash scripts/bootstrap.sh\n'
