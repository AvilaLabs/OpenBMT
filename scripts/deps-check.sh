#!/usr/bin/env bash
# Dependency check (docs/DEPENDENCIES.md).
#
# For each shipped program, the external packages it is built from (normal
# and build dependencies, with versions) must be exactly the list in
# deps/<program>.txt. Every package in both lock files must come from
# crates.io, and every external package's licence must be on the list in
# deps/licences.txt.
#
#   scripts/deps-check.sh           check; exit 1 on any difference
#   scripts/deps-check.sh --write   rewrite the lists from the lock file
#
# A changed list is a decision: the commit that changes one says why.
set -euo pipefail
cd "$(dirname "$0")/.."

PROGRAMS="openbmt-signer openbmt-counter-node openbmt-owner-node openbmt-box \
openbmt-books openbmt-key-ffi openbmt-counter-ffi openbmt-qr openbmt-rates"
write=0
[ "${1:-}" = "--write" ] && write=1
fail=0

externals() {
    CARGO_TERM_COLOR=never cargo tree --color never --locked --offline \
        -p "$1" -e normal,build --prefix none \
        -f '{p}' 2>/dev/null |
        sed 's/ (\*)$//' | grep -v ' (/' | grep -v '^$' |
        sed 's/ (proc-macro)$//' | sort -u
}

for p in $PROGRAMS; do
    list="deps/$p.txt"
    now="$(externals "$p" || true)"
    # A program with no external packages at all has an empty list.
    if [ -z "$now" ] && [ -f "$list" ] && [ ! -s "$list" ] &&
        cargo tree --locked --offline -p "$p" -e normal,build >/dev/null 2>&1; then
        if [ "$write" = 1 ]; then
            echo "WROTE $list (0 packages)"
        else
            echo "PASS  $p: 0 external packages, as listed"
        fi
        continue
    fi
    if [ -z "$now" ]; then
        echo "FAIL  $p: no packages found (is the name right?)"
        fail=1
        continue
    fi
    if [ "$write" = 1 ]; then
        printf '%s\n' "$now" >"$list"
        echo "WROTE $list ($(printf '%s\n' "$now" | wc -l) packages)"
    elif [ ! -f "$list" ]; then
        echo "FAIL  $p: $list is missing"
        fail=1
    elif ! diff -u "$list" <(printf '%s\n' "$now") >/dev/null; then
        echo "FAIL  $p: its packages are not the ones in $list"
        diff -u "$list" <(printf '%s\n' "$now") | sed -n '3,$p' | grep '^[+-]' || true
        fail=1
    else
        echo "PASS  $p: $(printf '%s\n' "$now" | wc -l) external packages, as listed"
    fi
done

# Sources: crates.io or the workspace itself, nothing else.
for lock in Cargo.lock fuzz/Cargo.lock; do
    other="$(grep '^source = ' "$lock" | sort -u |
        grep -v '"registry+https://github.com/rust-lang/crates.io-index"' || true)"
    if [ -n "$other" ]; then
        echo "FAIL  $lock: a package comes from somewhere other than crates.io:"
        echo "$other"
        fail=1
    else
        echo "PASS  $lock: every package is from crates.io"
    fi
done

# Licences of everything in the main lock file.
bad="$(cargo metadata --locked --offline --format-version 1 2>/dev/null |
    python3 -I -c '
import json, sys
allowed = [l.strip() for l in open("deps/licences.txt")
           if l.strip() and not l.startswith("#")]
meta = json.load(sys.stdin)
for p in meta["packages"]:
    if p["source"] is None:
        continue
    lic = p.get("license") or ""
    parts = lic.replace("(", " ").replace(")", " ").replace("/", " OR ")
    options = [o.strip() for o in parts.split(" OR ")]
    if not any(o in allowed for o in options):
        print("%s %s: %s" % (p["name"], p["version"], lic or "no licence stated"))
')"
if [ -n "$bad" ]; then
    echo "FAIL  licences not on deps/licences.txt:"
    echo "$bad"
    fail=1
else
    echo "PASS  every external package has a licence on deps/licences.txt"
fi

[ "$fail" = 0 ] && echo "ALL PASS" || { echo "FAILED"; exit 1; }
