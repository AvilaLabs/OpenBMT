#!/usr/bin/env bash
# Compares the key maker with Electrum's own library, offline.
#
# usage: crosscheck.sh <folder holding the extracted Electrum AppImage>
#
# Extract once with `electrum-4.8.4-x86_64.AppImage --appimage-extract`; the
# folder to pass is the resulting `squashfs-root`. Electrum runs without a
# network (bwrap --unshare-net) and its output must equal expected.txt.
# Run under the memory guard. Regenerate expected.txt with REGEN=1.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "${1:?folder of the extracted Electrum}" && pwd)
out=$(mktemp "${TMPDIR:-$HOME/.cache}/keymaker-crosscheck.XXXXXX")
trap 'rm -f "$out"' EXIT
bwrap --unshare-net --ro-bind / / --dev /dev --proc /proc \
  --setenv LD_LIBRARY_PATH "$root/usr/lib:$root/usr/lib/x86_64-linux-gnu" \
  "$root/usr/bin/python3" -s "$here/crosscheck.py" < "$here/flips.txt" > "$out"
if [ "${REGEN:-}" = 1 ]; then
  cp "$out" "$here/expected.txt"
fi
diff -u "$here/expected.txt" "$out"
echo "electrum cross-check: $(wc -l < "$out") flip strings match"
