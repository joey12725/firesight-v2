#!/usr/bin/env bash
# Makes the simulation engine available to the MCP function on Vercel: builds the wasm
# package if nothing has built it yet, then copies it to api/_engine/ (npm run engine).
#
# vercel.json names this as the install step of api/mcp.ts (through the npm script
# "vercel-engine"), so it runs just before the platform bundles that function, whichever
# of the function and the web build the platform happens to do first:
#   - web build first: web/src/engine/pkg/ is already there and this only copies it;
#   - function first: this builds the wasm, and the web build that follows finds it
#     compiled, because both use the same toolchain and cargo cache
#     (web/scripts/vercel-build.sh).
# Safe to run any number of times.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="$ROOT/web/src/engine/pkg"

if [ ! -f "$PKG/firesight_wasm_bg.wasm" ] || [ ! -f "$PKG/firesight_wasm.js" ] || [ ! -f "$PKG/firesight_wasm.d.ts" ]; then
  # wasm-pack is a devDependency of the web app.
  if [ ! -x "$ROOT/web/node_modules/.bin/wasm-pack" ]; then
    echo "==> Installing the web app's dependencies (for wasm-pack)"
    npm install --prefix "$ROOT/web" --no-audit --no-fund
  fi

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
  echo "==> Building the engine for the MCP function: $(rustc --version)"
  npm run wasm --prefix "$ROOT/web"
fi

cd "$ROOT"
npx --no-install tsx scripts/copy-engine.ts
