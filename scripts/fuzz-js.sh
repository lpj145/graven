#!/usr/bin/env bash
# Mutation fuzzing of the JavaScript runtime and generated code (ts/tests/fuzz.ts), with the
# Rust reference as oracle (crates/graven-conformance/src/bin/conformance-oracle.rs).
#
#   scripts/fuzz-js.sh            # 60 s, random seed
#   scripts/fuzz-js.sh 600        # 600 s
#   scripts/fuzz-js.sh 60 1234    # 60 s, seed 1234 (to replay a run)
#
# Failing inputs are saved in ts/fuzz-crashes/. Requires what scripts/ts.sh requires.
set -euo pipefail
cd "$(dirname "$0")/.."

seconds="${1:-60}"
seed="${2:-}"

cargo build -q -p graven-conformance --bin conformance-oracle
# Compiles the runtime, generates the test code and builds ts/dist.
scripts/ts.sh >/dev/null

args=(--seconds "$seconds")
[ -n "$seed" ] && args+=(--seed "$seed")
node ts/dist/tests/fuzz.js "${args[@]}"
