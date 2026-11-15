#!/usr/bin/env bash
# Builds dist/site for openbmt.avilalabs.org: the demo at the root and the handbook at /handbook/
# (mdbook, into dist/handbook).
#
# The demo is the page of demo/web2 (index.html) with the drawing layer of demo/ui beside it, the counter app's
# own model compiled for the browser (scripts/demo-kotlin-js.sh counter) and the counter's own library as
# WebAssembly (scripts/demo-build.sh counter), and the owner app's model and library the same way
# (scripts/demo-kotlin-js.sh owner; the owner's module is built by demo-build.sh too). The first page of the demo (demo/web) stays in the repository and is
# not in the site. The paths in index.html, which are the repository's, are written as the site's own.
set -euo pipefail
cd "$(dirname "$0")/.."

scripts/demo-build.sh counter
scripts/demo-kotlin-js.sh counter
scripts/demo-kotlin-js.sh owner
mdbook build

rm -rf dist/site
mkdir -p dist/site/ui dist/site/fonts
# The page, its scripts and style, with the repository's paths written as the site's.
for f in shell.js shell.css wallet.js payment-code.js questions.js one-chain.js counter-controller.js owner-controller.js live.js counter-host.mjs owner-host.mjs; do
    cp "demo/web2/$f" dist/site/
done
sed -e 's#\.\./\.\./dist/kotlin-js/counter/#./#g' -e 's#\.\./\.\./dist/demo-counter/#./#g' \
    -e 's#\.\./\.\./dist/kotlin-js/owner/#./#g' -e 's#\.\./\.\./dist/demo-owner/#./#g' \
    -e 's#\.\./ui/#ui/#g' -e 's#\.\./web/#./#g' demo/web2/index.html >dist/site/index.html
sed -i 's#\.\./web/fonts/#fonts/#g' dist/site/shell.css
cp demo/ui/ui.js demo/ui/chrome.js demo/ui/counter-views.js demo/ui/owner-views.js demo/ui/gallery-data.js \
    demo/ui/ui.css demo/ui/palette.json demo/ui/logo.png dist/site/ui/
cp -R demo/ui/fonts dist/site/ui/fonts
cp demo/web/favicon.svg dist/site/
cp demo/web/fonts/ubuntu-latin-*.woff2 demo/web/fonts/UBUNTU-FONT-LICENCE.txt dist/site/fonts/
cp dist/kotlin-js/counter/counter-model.mjs dist/site/
cp dist/demo-counter/counter.wasm dist/site/
cp dist/kotlin-js/owner/owner-model.mjs dist/site/
cp dist/demo-owner/owner.wasm dist/site/
cp -R dist/handbook dist/site/handbook
# A page that is not found is the handbook's own.
cp dist/handbook/404.html dist/site/404.html
cp deploy/handbook/_headers dist/site/_headers
echo "dist/site ready"
