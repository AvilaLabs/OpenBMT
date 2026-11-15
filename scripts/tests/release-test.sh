#!/usr/bin/env bash
# Tests of scripts/release-keys-make.sh, release-sign.sh (--sign-only) and release-verify.sh
# with throwaway keys. Nothing real is used: the keys, the gpg home and the artifacts are made
# in a temporary directory under ~/.cache/openbmt/tmp/release-test/, which is deleted at the end.
# The scripts take their passphrases from the prompts of keytool, apksigner and gpg; here
# OPENBMT_RELEASE_TEST=1 (refused outside that directory) gives them a test passphrase file
# and a gpg key without a passphrase.
set -uo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
root=$HOME/.cache/openbmt/tmp/release-test
mkdir -p "$root"
tmp=$(mktemp -d "$root/run.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
export JAVA_HOME="${JAVA_HOME:-$HOME/Android/jdk21}"
export PATH="$JAVA_HOME/bin:$PATH"
export TMPDIR=$tmp
export OPENBMT_RELEASE_TEST=1
export GNUPGHOME=$tmp/gnupg
mkdir -m 700 "$GNUPGHOME"

pass=0 failn=0
check() { # name, then the command; passes when it exits 0
  local name=$1; shift
  if "$@" >"$tmp/last.out" 2>&1; then pass=$((pass + 1)); echo "ok    $name"
  else failn=$((failn + 1)); echo "FAIL  $name"; sed -e 's/^/      /' "$tmp/last.out" | tail -n 15; fi
}
refuses() { # name, then the command; passes when it exits non-zero
  local name=$1; shift
  if "$@" >"$tmp/last.out" 2>&1; then failn=$((failn + 1)); echo "FAIL  $name (it succeeded)"
  else pass=$((pass + 1)); echo "ok    $name"; fi
}

# ---- keys -------------------------------------------------------------------------------
keydir=$tmp/keys
mkdir "$keydir"
echo 'test-only-passphrase' > "$keydir/test.pass"
"$here/release-keys-make.sh" "$keydir" >"$tmp/keys.out" 2>&1
check "keys: three p12 files" test -f "$keydir/openbmt-owner.p12" -a -f "$keydir/openbmt-counter.p12" -a -f "$keydir/openbmt-key.p12"
check "keys: ends with the sentence" grep -qxF "Copy $keydir to a second offline place. If these files are lost, the installed apps can never be updated." "$tmp/keys.out"
for app in owner counter key; do
  check "keys: $app fingerprint line" grep -qE "^$app: [0-9a-f]{64}\$" "$tmp/keys.out"
done
refuses "keys: never overwrites" "$here/release-keys-make.sh" "$keydir"
refuses "keys: refuses a directory in the repository" "$here/release-keys-make.sh" "$here/../dist/keys-test"
refuses "keys: test mode refused outside the test directory" env OPENBMT_RELEASE_TEST=1 "$here/release-keys-make.sh" "$HOME/.cache/openbmt/tmp/not-the-test-dir"
rmdir "$HOME/.cache/openbmt/tmp/not-the-test-dir" 2>/dev/null

# ---- gpg key without a passphrase -----------------------------------------------------------
gpg --batch --quiet --passphrase '' --quick-generate-key "OpenBMT release test <test@example.invalid>" rsa3072 sign never >/dev/null 2>&1
fpr=$(gpg --with-colons --fingerprint 2>/dev/null | awk -F: '$1 == "fpr" {print tolower($10); exit}')
check "gpg: test key made" test "${#fpr}" = 40

# ---- fake artifacts ---------------------------------------------------------------------------
version=0.0.1-test
work=$tmp/dist/work-$version
mkdir -p "$work" "$tmp/apkstage"
echo "stored file" > "$tmp/apkstage/data.txt"
zipalign=$HOME/Android/Sdk/build-tools/36.0.0/zipalign
aapt2=$HOME/Android/Sdk/build-tools/36.0.0/aapt2
cat > "$tmp/AndroidManifest.xml" <<'X'
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.openbmt.releasetest"><application/></manifest>
X
for app in owner counter key; do
  "$aapt2" link -o "$tmp/un-$app.apk" -I "$HOME/Android/Sdk/platforms/android-36/android.jar" \
    --manifest "$tmp/AndroidManifest.xml" --min-sdk-version 29 --version-code 1 --version-name 0.0.1 >/dev/null
  (cd "$tmp/apkstage" && echo "$app" > app.txt && zip -q -X "$tmp/un-$app.apk" data.txt app.txt)
  "$zipalign" -P 16 -f 4 "$tmp/un-$app.apk" "$work/aligned-$app.apk"
done
echo "fake box" > "$work/openbmt-box-$version-x86_64-linux"
echo "<html>fake</html>" > "$work/openbmt-keymaker-$version.html"
echo "commit 0123456789abcdef0123456789abcdef01234567" > "$work/COMMIT"

keys=$tmp/RELEASE-KEYS.md
for app in owner counter key; do eval "cert_$app=$(sed -n "s/^$app: //p" "$tmp/keys.out")"; done
write_keys() { # fingerprint owner counter key
  printf '# Release keys\n## GPG key of the release list\nfingerprint: %s\n## App signing keys\nowner: %s\ncounter: %s\nkey: %s\n' "$1" "$2" "$3" "$4" > "$keys"
}

# a wrong fingerprint in the list: the signing run refuses and removes the release
write_keys "$fpr" "$cert_owner" "$cert_counter" " $(printf 'a%.0s' $(seq 64))"
refuses "sign: mismatch with the list is refused" env OPENBMT_RELEASE_KEYS_FILE="$keys" "$here/release-sign.sh" --sign-only "$work" "$version" "$keydir" "$fpr"
check "sign: a refused release is removed" test ! -e "$tmp/dist/release-$version"

# a run with no fingerprints listed
printf '# Release keys\nfingerprint: (none yet)\nowner: (none yet)\ncounter: (none yet)\nkey: (none yet)\n' > "$tmp/empty.md"
check "sign: no fingerprints listed" env OPENBMT_RELEASE_KEYS_FILE="$tmp/empty.md" "$here/release-sign.sh" --sign-only "$work" "$version" "$keydir" "$fpr"
cp "$tmp/last.out" "$tmp/sign.out"
check "sign: says none are listed" grep -q "no fingerprints are listed yet" "$tmp/sign.out"
rel=$tmp/dist/release-$version
check "sign: files made" bash -c "cd '$rel' && ls openbmt-owner-$version.apk openbmt-counter-$version.apk openbmt-key-$version.apk openbmt-box-$version-x86_64-linux openbmt-keymaker-$version.html COMMIT SHA256SUMS SHA256SUMS.asc"
refuses "sign: never overwrites a release" env OPENBMT_RELEASE_KEYS_FILE="$tmp/empty.md" "$here/release-sign.sh" --sign-only "$work" "$version" "$keydir" "$fpr"
refuses "sign: bad version" "$here/release-sign.sh" --sign-only "$work" "1.0" "$keydir"

# ---- verify ----------------------------------------------------------------------------------------
write_keys "$fpr" "$cert_owner" "$cert_counter" "$cert_key"
check "verify: a good release passes" "$here/release-verify.sh" "$rel" "$keys"
cp "$tmp/last.out" "$tmp/verify.out"
check "verify: also takes 'app hex' without a colon" bash -c "sed 's/^key:/key/' '$keys' > '$tmp/k2.md'; '$here/release-verify.sh' '$rel' '$tmp/k2.md'"
check "verify: prints the expected fingerprint" grep -q "expected gpg key fingerprint: $fpr" "$tmp/verify.out"

copy() { rm -rf "$tmp/t"; cp -r "$rel" "$tmp/t"; }
copy; printf 'x' >> "$tmp/t/openbmt-owner-$version.apk"
refuses "verify: a changed byte in an APK" "$here/release-verify.sh" "$tmp/t" "$keys"
copy; sed -i '1s/^./0/;1s/^0\{2\}/1/' "$tmp/t/SHA256SUMS"
refuses "verify: a changed SHA256SUMS" "$here/release-verify.sh" "$tmp/t" "$keys"
copy; echo "extra" > "$tmp/t/extra.txt"
refuses "verify: an extra file" "$here/release-verify.sh" "$tmp/t" "$keys"
copy; rm "$tmp/t/SHA256SUMS.asc"
refuses "verify: no signature" "$here/release-verify.sh" "$tmp/t" "$keys"
copy; echo "tampered" >> "$tmp/t/COMMIT"
refuses "verify: a changed COMMIT" "$here/release-verify.sh" "$tmp/t" "$keys"
write_keys "$(printf 'b%.0s' $(seq 40))" "$cert_owner" "$cert_counter" "$cert_key"
refuses "verify: another gpg fingerprint listed" "$here/release-verify.sh" "$rel" "$keys"
write_keys "$fpr" "$cert_owner" "$(printf 'c%.0s' $(seq 64))" "$cert_key"
refuses "verify: another APK certificate listed" "$here/release-verify.sh" "$rel" "$keys"
write_keys "$fpr" "$cert_owner" "$cert_counter" "$cert_key"; printf 'owner: %s\n' "$cert_owner" >> "$keys"
refuses "verify: a label listed twice" "$here/release-verify.sh" "$rel" "$keys"
refuses "verify: nothing listed yet" "$here/release-verify.sh" "$rel" "$tmp/empty.md"
refuses "verify: the repository's own empty list" "$here/release-verify.sh" "$rel"

# a second gpg key signs a list that is otherwise intact
gpg --batch --quiet --passphrase '' --quick-generate-key "Other test <other@example.invalid>" rsa3072 sign never >/dev/null 2>&1
other=$(gpg --with-colons --fingerprint other@example.invalid 2>/dev/null | awk -F: '$1 == "fpr" {print tolower($10); exit}')
write_keys "$fpr" "$cert_owner" "$cert_counter" "$cert_key"
copy; rm "$tmp/t/SHA256SUMS.asc"; (cd "$tmp/t" && gpg --batch --armor --detach-sign --local-user "$other" SHA256SUMS 2>/dev/null)
refuses "verify: signed by another gpg key" "$here/release-verify.sh" "$tmp/t" "$keys"

echo
echo "release-test: $pass passed, $failn failed"
[ "$failn" = 0 ]
