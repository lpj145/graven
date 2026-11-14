#!/usr/bin/env bash
# Miri (nightly) over the runtime, the generated code and the compiler. Usage: scripts/miri.sh
#
# Under `cfg(miri)` the property tests and mutation gates use smaller samples (same code,
# fewer cases); no test exercising `Verified`, `VListR`, `VStrList`, `str_unchecked` or
# the envelope is skipped. Takes ~25 min on a desktop machine.
set -eu
cd "$(dirname "$0")/.."
# Isolation disabled: proptest reads/writes regression files (not graven code).
export MIRIFLAGS="${MIRIFLAGS:--Zmiri-strict-provenance -Zmiri-disable-isolation}"
export PROPTEST_CASES="${PROPTEST_CASES:-8}"
PKGS=${MIRI_PKGS:-"graven graven-layout graven-schemas graven-codegen-tests graven-conformance"}
cargo +nightly miri test $(printf -- '-p %s ' $PKGS)
# The compiler has no `unsafe`. Left out of Miri: `cli.rs` (spawns processes, unsupported)
# and `codegen.rs` (formats thousands of lines with prettyplease: hours under Miri).
if [ -z "${MIRI_PKGS:-}" ] || [ "${MIRI_COMPILER:-1}" = 1 ]; then
  cargo +nightly miri test -p graven-compiler --lib --test parse --test check --test compat --test lock --test names
fi
