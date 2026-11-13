#!/usr/bin/env bash
# Runs every test of the plugin's classes with the php command line alone: no
# web server, no WordPress. Prints each file's pass count and the total; the
# status is 1 if any check fails or any file does not run.
set -u
cd "$(dirname "$0")"

total=0
failed=0
for t in test_*.php; do
    out="$(php -d display_errors=stderr -d error_reporting=-1 "$t" 2>&1)"
    status=$?
    printf '%s\n' "$out"
    count="$(printf '%s\n' "$out" | sed -n 's/^PASS \([0-9][0-9]*\) .*/\1/p' | head -1)"
    if [ "$status" -ne 0 ] || [ -z "$count" ]; then
        echo "FAIL  $t"
        failed=1
    else
        total=$((total + count))
    fi
done

if [ "$failed" -ne 0 ]; then
    echo "FAILED after $total passing checks"
    exit 1
fi
echo "ALL PASS: $total checks"
