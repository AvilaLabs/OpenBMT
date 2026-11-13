#!/usr/bin/env bash
# Every framed encoding starts with four bytes of its own ("OBM" and one
# more). This checks that no two constants in the crates share them, so a
# file or message of one kind can never be read as another. All 62 letters
# and digits after "OBM" and all the printable punctuation are taken: a new
# one is a control byte written as \x1e (a hexadecimal escape in the source).
# A backslash or a double quote is written escaped (\\ and \").
set -euo pipefail
cd "$(dirname "$0")/.."

defs="$(grep -rnE '^\s*(pub(\([a-z]+\))? )?const [A-Z0-9_]+: \[u8; 4\] = \*b"OBM([^"\\]|\\\\|\\"|\\x[0-9a-f]{2})"' \
  crates --include='*.rs' || true)"
count="$(printf '%s\n' "$defs" | grep -c . || true)"
dups="$(printf '%s\n' "$defs" | grep -oE 'b"OBM([^"\\]|\\\\|\\"|\\x[0-9a-f]{2})"' | sort | uniq -d || true)"

if [ "$count" -lt 30 ]; then
  echo "FAIL  only $count magic constants found: the pattern no longer matches"
  exit 1
fi
if [ -n "$dups" ]; then
  echo "FAIL  magics defined more than once:"
  for d in $dups; do
    printf '%s\n' "$defs" | grep -F "$d"
  done
  exit 1
fi
echo "PASS  $count magic constants, all different"
