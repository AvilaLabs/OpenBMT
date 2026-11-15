#!/usr/bin/env bash
# Checks a downloaded release.
#
#   scripts/release-verify.sh <dir> [keys-file]
#
# <dir> holds the release's files. keys-file defaults to docs/RELEASE-KEYS.md,
# whose lines are "fingerprint: <40 hex>" and "<app>: <64 hex>" for owner,
# counter and key (the colon is optional, hex of either case, exactly one line
# each). Checks, one line each:
#   - SHA256SUMS.asc is a good gpg signature of SHA256SUMS made by the key whose
#     primary fingerprint is listed (importing that key into gpg is your step;
#     the script says which fingerprint it expects)
#   - SHA256SUMS lists every file of the directory (but itself and its signature) and nothing else
#   - sha256sum -c SHA256SUMS
#   - each APK's signing certificate SHA-256 is the one listed for its app
# Needs gpg and apksigner (Android build-tools; ANDROID_SDK or APKSIGNER).
# Exits 0 only if every check passes.
set -uo pipefail
release_name=release-verify.sh
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/release-lib.sh
. "$here/release-lib.sh"

[ $# -ge 1 ] && [ $# -le 2 ] || { echo "usage: scripts/release-verify.sh <dir> [keys-file]" >&2; exit 2; }
dir=$1
keys=${2:-$here/../docs/RELEASE-KEYS.md}
[ -d "$dir" ] || release_die "no directory $dir"
command -v gpg >/dev/null || release_die "gpg is needed"
command -v sha256sum >/dev/null || release_die "sha256sum is needed"

fail=0
ok() { printf 'ok    %s\n' "$1"; }
bad() { printf 'FAIL  %s\n' "$1"; fail=1; }

want_fpr=$(release_keys_value "$keys" fingerprint) || exit 1
if [ -z "$want_fpr" ]; then
  bad "gpg key: $(basename "$keys") lists no fingerprint yet, nothing to check the signature against"
else
  echo "expected gpg key fingerprint: $want_fpr (import that key into gpg first)"
  if [ -f "$dir/SHA256SUMS" ] && [ -f "$dir/SHA256SUMS.asc" ]; then
    status=$(gpg --status-fd 1 --verify "$dir/SHA256SUMS.asc" "$dir/SHA256SUMS" 2>/dev/null)
    if ! printf '%s\n' "$status" | grep -q '^\[GNUPG:\] GOODSIG '; then
      bad "SHA256SUMS.asc is not a good signature of SHA256SUMS (or its key is not imported, expired or revoked)"
    elif [ "$(printf '%s\n' "$status" | awk '$2 == "VALIDSIG" {print tolower($NF)}')" = "$want_fpr" ]; then
      ok "SHA256SUMS.asc is a good signature by $want_fpr"
    else
      bad "SHA256SUMS.asc is signed by a key other than $want_fpr"
    fi
  else
    bad "SHA256SUMS or SHA256SUMS.asc is missing"
  fi
fi

if [ -f "$dir/SHA256SUMS" ]; then
  listed=$(sed -E 's/^[0-9a-f]{64} [ *]//' "$dir/SHA256SUMS" | LC_ALL=C sort)
  present=$(cd "$dir" && find . -mindepth 1 -maxdepth 1 -type f ! -name SHA256SUMS ! -name SHA256SUMS.asc -printf '%f\n' | LC_ALL=C sort)
  if [ "$(find "$dir" -mindepth 1 ! -type f | grep -c .)" != 0 ]; then
    bad "the directory holds something that is not a plain file"
  elif [ -n "$listed" ] && [ "$listed" = "$present" ] && ! grep -qvE '^[0-9a-f]{64} [ *][^/]+$' "$dir/SHA256SUMS"; then
    ok "SHA256SUMS lists every file of the directory and nothing else"
  else
    bad "SHA256SUMS does not list exactly the files of the directory"
  fi
  if (cd "$dir" && sha256sum --strict -c SHA256SUMS >/dev/null 2>&1); then
    ok "sha256sum -c SHA256SUMS"
  else
    bad "sha256sum -c SHA256SUMS (a file differs from its listed hash)"
  fi
else
  bad "SHA256SUMS is missing"
fi

for app in $release_apps; do
  want=$(release_keys_value "$keys" "$app") || exit 1
  apks=$(find "$dir" -maxdepth 1 -type f -name "openbmt-$app-*.apk")
  if [ -z "$want" ]; then
    bad "$app APK: $(basename "$keys") lists no certificate fingerprint yet"
  elif [ "$(printf '%s' "$apks" | grep -c .)" != 1 ]; then
    bad "$app APK: expected exactly one openbmt-$app-*.apk in the directory"
  else
    cert=$(release_cert_sha256 "$apks") || { bad "$app APK: no certificate could be read"; continue; }
    if [ "$cert" = "$want" ]; then ok "$app APK certificate SHA-256 is $cert"
    else bad "$app APK certificate SHA-256 is $cert, $(basename "$keys") lists $want"; fi
  fi
done

[ $fail = 0 ] && echo "all checks passed" || echo "NOT VERIFIED"
exit $fail
