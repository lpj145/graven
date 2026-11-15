#!/usr/bin/env bash
# JavaScript/TypeScript checks:
#   1. compile the runtime (crates/graven-compiler/runtime/graven.ts) with tsc and check that
#      the committed graven.js / graven.d.ts are current (`--update-runtime` rewrites them);
#   2. generate `--lang js --side client` code for every test schema;
#   3. type-check the TypeScript tests under `tsc --strict` against the generated `.d.ts`;
#   4. run the Node suite (conformance v1, canonical writing, rpc-v1, validation,
#      declarations matching code, Rust <-> JS interop) and the plain JavaScript tests.
# Requires: Node >= 20, npm (for the TypeScript compiler in ts/node_modules).
set -euo pipefail
cd "$(dirname "$0")/.."

update_runtime=0
for arg in "$@"; do
  case "$arg" in
    --update-runtime) update_runtime=1 ;;
    *) echo "usage: scripts/ts.sh [--update-runtime]" >&2; exit 2 ;;
  esac
done

if ! command -v node >/dev/null 2>&1; then
  echo "error: node is required to run scripts/ts.sh" >&2
  exit 1
fi

if [ ! -x ts/node_modules/.bin/tsc ]; then
  echo "==> installing TypeScript devDependency in ts/"
  (cd ts && npm ci --no-audit --no-fund)
fi

echo "==> compiling the runtime"
rm -rf ts/out-runtime
(cd ts && ./node_modules/.bin/tsc -p tsconfig.runtime.json)
rt=crates/graven-compiler/runtime
for f in graven.js graven.d.ts; do
  if ! cmp -s "ts/out-runtime/$f" "$rt/$f"; then
    if [ "$update_runtime" = 1 ]; then
      cp "ts/out-runtime/$f" "$rt/$f"
      echo "    updated $rt/$f"
    else
      echo "error: $rt/$f is out of date with graven.ts; run scripts/ts.sh --update-runtime" >&2
      exit 1
    fi
  fi
done

echo "==> building gravenc and inventory_stdio"
cargo build -p graven-compiler --bin gravenc
cargo build -p graven-codegen-tests --bin inventory_stdio

rm -rf ts/tests/gen ts/dist
mkdir -p ts/tests/gen

echo "==> generating JavaScript (--lang js --side client)"
for schema in \
  crates/graven-conformance/schemas/conformance.graven \
  crates/graven-codegen-tests/schemas/*.graven \
  crates/graven-schemas/schemas/*.graven
do
  ./target/debug/gravenc gen "$schema" -o ts/tests/gen --lang js --side client >/dev/null
done

echo "==> type-checking the tests with tsc --strict"
(cd ts && ./node_modules/.bin/tsc)
# The tests are compiled into ts/dist; the generated code they import is already JavaScript.
mkdir -p ts/dist/tests/gen
cp ts/tests/gen/*.js ts/dist/tests/gen/

echo "==> running the Node test suite"
INVENTORY_STDIO_BIN="$(pwd)/target/debug/inventory_stdio" \
  node --test ts/dist/tests/*.test.js ts/tests/*.test.mjs
