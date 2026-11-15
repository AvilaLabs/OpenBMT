#!/usr/bin/env bash
# Builds dist/site for openbmt.avilalabs.org: the demo at the root
# (scripts/demo-build.sh, into dist/demo) and the handbook at /handbook/
# (mdbook, into dist/handbook).
set -euo pipefail
cd "$(dirname "$0")/.."

scripts/demo-build.sh
mdbook build

rm -rf dist/site
mkdir -p dist/site
cp -R dist/demo/. dist/site/
cp -R dist/handbook dist/site/handbook
# A page that is not found is the handbook's own.
cp dist/handbook/404.html dist/site/404.html
cp deploy/handbook/_headers dist/site/_headers
echo "dist/site ready"
