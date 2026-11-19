#!/usr/bin/env bash
# Builds the native CLI with profile-guided optimisation (optional; native only).
#
#   tools/pgo-build.sh            # result: target/pgo/release/firesight
#
# Three steps: an instrumented build, a short training run of the three shipped
# examples (1,500 lives each, one thread), and a rebuild that uses the profile.
# The simulation is 10 to 20% faster than the plain release build; results are
# the same to the bit (the profile changes inlining and code layout, never
# arithmetic). `tools/bench-check.sh` and the tests use the plain build.
#
# Needs `llvm-profdata` of the same LLVM major version as rustc. Either
#   rustup component add llvm-tools
# or any LLVM install on PATH (set LLVM_PROFDATA to point at a specific binary).
set -euo pipefail
cd "$(dirname "$0")/.."

profdata="${LLVM_PROFDATA:-}"
if [ -z "$profdata" ]; then
  host="$(rustc -vV | sed -n 's/^host: //p')"
  candidate="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-profdata"
  if [ -x "$candidate" ]; then
    profdata="$candidate"
  elif command -v llvm-profdata >/dev/null 2>&1; then
    profdata="$(command -v llvm-profdata)"
  else
    echo "llvm-profdata not found: run 'rustup component add llvm-tools' or set LLVM_PROFDATA" >&2
    exit 1
  fi
fi

# RUSTFLAGS replaces the flags in .cargo/config.toml, so they are repeated here.
base_flags="$(sed -n '/^\[build\]/,/^\[target/s/^rustflags = \["\(.*\)"\]$/\1/p' .cargo/config.toml | sed 's/", "/ /g')"
data="$PWD/target/pgo/profiles"
rm -rf "$data"

echo "1/3 instrumented build"
RUSTFLAGS="$base_flags -Cprofile-generate=$data" \
  cargo build --release -p firesight-cli --target-dir target/pgo/instrumented

echo "2/3 training run"
for example in examples/*.json; do
  target/pgo/instrumented/release/firesight run "$example" --paths 1500 --threads 1 >/dev/null
done
"$profdata" merge -o target/pgo/merged.profdata "$data"

echo "3/3 optimised build"
RUSTFLAGS="$base_flags -Cprofile-use=$PWD/target/pgo/merged.profdata" \
  cargo build --release -p firesight-cli --target-dir target/pgo

echo "built target/pgo/release/firesight"
target/pgo/release/firesight bench examples/dual-income-family.json --paths 8000 | tail -1
