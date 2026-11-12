#!/usr/bin/env bash
# A reproducible release of the key maker page.
#
#   scripts/keymaker-release.sh <version>
#
# Builds tools/keymaker from the checked-out commit, which must be clean, with
# a fixed environment, and writes
#   tools/keymaker/dist/release/openbmt-keymaker-<version>.html
#   tools/keymaker/dist/release/SHA256SUMS
# then prints the hash and the commit. Anyone can check out the same commit,
# run this script and compare SHA256SUMS. Needs Node 22.22.1 (the version the
# CI job uses) and the pinned packages in the npm cache or on the network.
# Signing is not done here (see sign_release).
set -euo pipefail

NODE_PINNED=22.22.1

version=${1:-}
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || [[ $# -ne 1 ]]; then
  echo "usage: scripts/keymaker-release.sh <version>   (for example 0.1.0)" >&2
  exit 2
fi

top=$(cd "$(dirname "${BASH_SOURCE[0]}")" && git rev-parse --show-toplevel)
cd "$top"
if [[ -n $(git status --porcelain --untracked-files=all) ]]; then
  echo "refused: the working tree is not clean; check out a commit and remove changes" >&2
  exit 1
fi
node_now=$(node --version 2>/dev/null || true)
if [[ $node_now != "v$NODE_PINNED" ]]; then
  echo "refused: Node $NODE_PINNED is required, found '${node_now:-none}'" >&2
  exit 1
fi

commit=$(git rev-parse HEAD)
epoch=$(git log -1 --format=%ct HEAD)

# Fixed environment: nothing of the machine's locale, zone, user or path goes
# into the page.
export TZ=UTC LC_ALL=C LANG=C
export SOURCE_DATE_EPOCH=$epoch
export KEYMAKER_VERSION=$version
export KEYMAKER_COMMIT=${commit:0:12}

cd tools/keymaker
npm ci --ignore-scripts
rm -rf dist
node build.mjs
name=openbmt-keymaker-$version.html
mkdir -p dist/release
cp dist/openbmt-keymaker.html "dist/release/$name"
(cd dist/release && sha256sum "$name" > SHA256SUMS)

cat dist/release/SHA256SUMS
echo "commit $commit"

# Signing a release is done separately: a detached signature over SHA256SUMS,
# made by the maintainer with a key that never touches this script.
sign_release() {
  echo "signing is done separately"
  exit 0
}
sign_release
