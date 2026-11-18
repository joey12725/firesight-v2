#!/usr/bin/env bash
# Vercel build: make a Rust toolchain available, then run the normal web build
# (wasm -> schema + types -> data -> tsc -> vite). Runs from the repository root.
#
# Vercel's build image has no Rust. The toolchain and cargo's build output are kept
# under web/node_modules/.cache, which Vercel restores between builds (the install
# command is `npm install`, which leaves that directory alone), so only the first
# build pays for the download and the full compile. wasm-pack itself is an npm
# devDependency and needs no separate install.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CACHE="${FIRESIGHT_BUILD_CACHE:-$ROOT/web/node_modules/.cache/firesight}"
mkdir -p "$CACHE"

export RUSTUP_HOME="${RUSTUP_HOME:-$CACHE/rustup}"
export CARGO_HOME="${CARGO_HOME:-$CACHE/cargo}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$CACHE/target}"
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_TERM_COLOR=never
export CARGO_INCREMENTAL=0

if ! command -v cargo >/dev/null 2>&1; then
  echo "==> Installing Rust (minimal profile)"
  curl --proto '=https' --tlsv1.2 --retry 3 -sSf https://sh.rustup.rs |
    sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable --target wasm32-unknown-unknown
fi
rustup target add wasm32-unknown-unknown >/dev/null
echo "==> $(rustc --version), $(cargo --version)"

cd "$ROOT/web"
npm run build
