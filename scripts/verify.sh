#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

printf '\n== FileFlow verification ==\n'
printf 'Node:  %s\n' "$(node --version)"
printf 'pnpm:  %s\n' "$(pnpm --version)"
printf 'Rust:  %s\n\n' "$(rustc --version)"

printf '%s\n' '1/6 Angular production build'
pnpm run frontend:build

printf '\n%s\n' '2/6 Angular tests'
pnpm run frontend:test

printf '\n%s\n' '3/6 Rust formatting'
cargo fmt --all -- --check

printf '\n%s\n' '4/6 Rust workspace check'
cargo check --workspace --locked

printf '\n%s\n' '5/6 Rust tests'
cargo test --workspace --locked

printf '\n%s\n' '6/6 Clippy (warnings are errors)'
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

printf '\nFileFlow verification passed.\n'
