#!/usr/bin/env bash
# Build the website into target/site/ (static files, ready for Cloudflare Pages or any
# static host): the landing page and its translations from site/, the documentation from
# docs/ under docs/ (with the binary layout examples encoded by gravenc), the rustdoc API
# reference under api/, and the v1 conformance vectors as a .tar.gz.
# Usage: scripts/site.sh        Requires: Python 3 with site/requirements.txt
set -eu
cd "$(dirname "$0")/.."
python3 -c "import markdown_it, pygments" 2>/dev/null || { echo "missing Python packages: pip install -r site/requirements.txt" >&2; exit 1; }

rm -rf target/site

python3 site/build.py target/site
cargo build -q -p graven-compiler --bin gravenc
python3 site/layouts.py target/debug/gravenc target/site/assets/layouts.json
rm -rf target/doc
cargo doc -q -p graven -p graven-layout -p graven-dynamic -p graven-compiler --no-deps
cp -r target/doc target/site/api
tar -czf target/site/graven-testvectors-v1.tar.gz -C testvectors v1
echo "site: target/site ($(du -sh target/site | cut -f1))"
