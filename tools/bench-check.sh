#!/usr/bin/env bash
# Throughput regression check (docs/architecture.md section 7).
#
#   tools/bench-check.sh            # compare with tools/bench-baseline.json; exit 1 on a regression
#   tools/bench-check.sh --record   # re-record the baseline on this machine
#
# Builds the release CLI and runs `firesight bench` on the four shipped examples.
# Each run prints its one-line JSON summary last. The check compares a score that
# is normalised by a calibration kernel timed in the same process (see
# crates/firesight-cli/src/bench.rs), so it follows the code, not the machine, and
# every measurement is the best of several runs, so other load on the machine
# does not fail it. A run fails only when a score is more than the baseline's
# tolerance (25%) below the recorded one.
#
# How it is meant to run: on every change to crates/, on the same class of
# machine the baseline was recorded on (the normalisation absorbs differences in
# clock speed and load, not differences between CPU families). After an
# intended change in speed, or on a new runner class, run with --record and
# commit tools/bench-baseline.json. BENCH_PATHS and BENCH_REPEAT change the sample
# (defaults 8000 and 5: about 15 seconds in all).
set -euo pipefail
cd "$(dirname "$0")/.."

baseline="tools/bench-baseline.json"
paths="${BENCH_PATHS:-8000}"
repeat="${BENCH_REPEAT:-5}"
mode="check"
if [ "${1:-}" = "--record" ]; then
  mode="record"
fi

cargo build --release -p firesight-cli
machine="$(uname -sm), $(getconf _NPROCESSORS_ONLN 2>/dev/null || echo '?') cores"

status=0
for example in examples/dual-income-family.json examples/early-career-saver.json examples/side-hustle-landlord.json examples/startup-employee.json; do
  echo "== $example"
  if [ "$mode" = "record" ]; then
    target/release/firesight bench "$example" --paths "$paths" --repeat "$repeat" \
      --write-baseline "$baseline" --machine "$machine"
  else
    target/release/firesight bench "$example" --paths "$paths" --repeat "$repeat" \
      --check "$baseline" || status=1
  fi
done

if [ "$status" -ne 0 ]; then
  echo "bench-check: throughput regressed by more than the tolerance in $baseline" >&2
fi
exit "$status"
