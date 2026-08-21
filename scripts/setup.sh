#!/usr/bin/env bash
set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
case "$(uname -s 2>/dev/null || printf unknown)" in
  Darwin) exec bash "$SCRIPT_DIR/setup-macos.sh" "$@" ;;
  Linux) exec bash "$SCRIPT_DIR/setup-linux.sh" "$@" ;;
  *) echo 'Unsupported development host. Windows uses install.ps1/runtime scripts.' >&2; exit 1 ;;
esac
