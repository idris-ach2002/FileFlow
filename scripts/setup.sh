#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OS=$(uname -s 2>/dev/null || printf unknown)

case "$OS" in
  Darwin)
    exec sh "$SCRIPT_DIR/setup-macos.sh" "$@"
    ;;
  Linux)
    exec sh "$SCRIPT_DIR/setup-linux.sh" "$@"
    ;;
  *)
    echo "Unsupported host: $OS"
    echo "FileFlow development currently supports macOS and Linux."
    exit 1
    ;;
esac
