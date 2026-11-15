#!/usr/bin/env bash
# Makes the three Android signing keys of the apps.
#
#   scripts/release-keys-make.sh <keydir>
#
# One PKCS12 file per app, <keydir>/openbmt-<app>.p12 with the alias
# openbmt-<app>, for owner, counter and key: RSA 3072 (so a rebuild can match
# byte for byte, see scripts/repro-check.sh), valid 10000 days. The Key app's
# key is a file of its own so that it can go to a second person later.
# keytool asks for each store passphrase; this script never takes one as an
# argument, a variable or a file. <keydir> must be outside the repository and
# no key already there is overwritten.
set -euo pipefail
release_name=release-keys-make.sh
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/release-lib.sh
. "$here/release-lib.sh"

[ $# -eq 1 ] || { echo "usage: scripts/release-keys-make.sh <keydir>" >&2; exit 2; }
release_tools
release_check_keydir "$1"
keydir=$release_keydir
for app in $release_apps; do
  [ ! -e "$keydir/openbmt-$app.p12" ] || release_die "refused: $keydir/openbmt-$app.p12 already exists; a key is never overwritten"
done
umask 077

store_opt() { if [ "$release_test" = 1 ]; then printf '%s\n' "-storepass:file" "$keydir/test.pass"; fi; }
cap() { local w=$1; printf '%s%s' "$(printf '%s' "${w:0:1}" | tr a-z A-Z)" "${w:1}"; }

for app in $release_apps; do
  mapfile -t so < <(store_opt)
  keytool -genkeypair -keystore "$keydir/openbmt-$app.p12" -storetype PKCS12 \
    -keyalg RSA -keysize 3072 -validity 10000 -alias "openbmt-$app" \
    -dname "CN=OpenBMT $(cap "$app") release, O=OpenBMT" "${so[@]}"
done

echo
echo "Certificate SHA-256 fingerprints (for docs/RELEASE-KEYS.md):"
for app in $release_apps; do
  mapfile -t so < <(store_opt)
  fp=$(keytool -J-Duser.language=en -list -v -keystore "$keydir/openbmt-$app.p12" -storetype PKCS12 \
    -alias "openbmt-$app" "${so[@]}" | sed -n 's/^[[:space:]]*SHA256:[[:space:]]*//p' | tr -d ':' | tr 'A-F' 'a-f')
  printf '%s' "$fp" | grep -qE '^[0-9a-f]{64}$' || release_die "could not read the fingerprint of the $app key"
  echo "$app: $fp"
done
echo
echo "Copy $keydir to a second offline place. If these files are lost, the installed apps can never be updated."
