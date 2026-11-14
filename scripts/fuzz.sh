#!/usr/bin/env bash
# Fuzz graven with cargo-fuzz (nightly), optimized with debug assertions and overflow
# checks (`-O -a`): integer overflow panics instead of silently wrapping.
# Usage: scripts/fuzz.sh [seconds=600] [target...]
#
# Runs the targets in parallel, each for N seconds. Seed corpus: testvectors/v1/*.bin
# (byte targets) and the repository's .graven files (`schema` target). Exits non-zero if any
# target finds a crash; the artifact lands in fuzz/artifacts/<target>/.
#
# Before a release: >= 1 h per target.
set -u
cd "$(dirname "$0")/.."
S=${1:-600}
shift || true
TARGETS=("$@")
[ ${#TARGETS[@]} -eq 0 ] && TARGETS=(read verify envelope differential schema)

for t in "${TARGETS[@]}"; do
  mkdir -p "fuzz/corpus/$t"
  if [ "$t" = schema ]; then
    for f in $(find crates -name '*.graven' -not -path '*/target/*'); do cp "$f" "fuzz/corpus/$t/$(basename "$f")"; done
  else
    cp testvectors/v1/*.bin "fuzz/corpus/$t/"
  fi
done

(cd fuzz && cargo +nightly fuzz build -O -a) || exit 2

pids=()
for t in "${TARGETS[@]}"; do
  (cd fuzz && cargo +nightly fuzz run -O -a "$t" "corpus/$t" -- -max_total_time="$S" -rss_limit_mb=2048 -print_final_stats=1 \
     > "../target/fuzz-$t.log" 2>&1) &
  pids+=($!)
done
fail=0
for i in "${!TARGETS[@]}"; do
  t=${TARGETS[$i]}
  if wait "${pids[$i]}"; then
    echo "fuzz $t: ok ($(grep -m1 -o 'stat::number_of_executed_units: *[0-9]*' "target/fuzz-$t.log" | grep -o '[0-9]*$') executions)"
  else
    fail=1
    echo "fuzz $t: FINDING — log: target/fuzz-$t.log"
    grep -E 'Test unit written to|panicked' "target/fuzz-$t.log" | head -5
  fi
done
exit $fail
