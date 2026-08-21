#!/bin/sh
set -eu

PNPM_VERSION="11.20.0"

printf 'FileFlow bootstrap on %s / %s\n' "$(uname -s 2>/dev/null || echo unknown)" "$(uname -m 2>/dev/null || echo unknown)"

if ! command -v node >/dev/null 2>&1; then
  echo "Node.js is required. See .nvmrc for the recommended version."
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust/Cargo is required. Install Rust with rustup first."
  exit 1
fi

if [ "$(uname -s 2>/dev/null || true)" = "Darwin" ] && ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode Command Line Tools are required on macOS. Run: xcode-select --install"
  exit 1
fi

printf 'Node:  %s\n' "$(node --version)"
printf 'Rust:  %s\n' "$(rustc --version)"
printf 'Cargo: %s\n' "$(cargo --version)"

# FileFlow pins pnpm through package.json. Node 22 ships Corepack, but its
# shims are disabled by default. Enabling them avoids relying on the host npm
# resolver and gives every contributor the same pnpm version.
if ! command -v corepack >/dev/null 2>&1; then
  echo "Corepack was not found. FileFlow expects Node.js from .nvmrc (Node 22), which includes Corepack."
  exit 1
fi

corepack enable
corepack install --global "pnpm@${PNPM_VERSION}" >/dev/null 2>&1 || true

printf 'pnpm: %s\n' "$(pnpm --version)"

# Clean only leftovers created by npm. A valid pnpm virtual store is preserved
# so running bootstrap repeatedly stays fast and deterministic.
if [ -f package-lock.json ] || [ -f frontend/package-lock.json ]; then
  rm -f package-lock.json frontend/package-lock.json
fi
if [ -d node_modules ] && [ ! -d node_modules/.pnpm ]; then
  echo "Removing a non-pnpm root node_modules directory..."
  rm -rf node_modules
fi

pnpm install --frozen-lockfile
cargo fetch --locked
sh scripts/check-engines.sh

echo
echo "FileFlow dependencies are ready. Run: pnpm run dev"
