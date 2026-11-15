#!/usr/bin/env bash
# Builds a release from the checked-out commit and signs it.
#
#   scripts/release-sign.sh <version> <keydir> [gpg-key]
#   scripts/release-sign.sh --sign-only <workdir> <version> <keydir> [gpg-key]
#
# Builds the three APKs (apps/*/build.sh, stopped after the aligned APK), the
# Box binary (scripts/box-build.sh) and the key maker page
# (scripts/keymaker-release.sh) from a clean working tree, with
# SOURCE_DATE_EPOCH the commit time as scripts/repro-check.sh does, into
# dist/work-<version>/. Then signs each APK with the app's key from <keydir>
# (made by scripts/release-keys-make.sh) and writes dist/release-<version>/:
#   openbmt-{owner,counter,key}-<version>.apk
#   openbmt-box-<version>-x86_64-linux   (the binary cargo builds for this x86_64 Linux host)
#   openbmt-keymaker-<version>.html
#   COMMIT   (the line "commit <hash>")
#   SHA256SUMS, and SHA256SUMS.asc (a detached, armored gpg signature of it)
# apksigner and gpg ask for the passphrases; none is taken from an argument, a
# variable or a file (test mode excepted, see scripts/release-lib.sh).
#
# --sign-only skips the build and the clean check and signs what is in
# <workdir>: aligned-{owner,counter,key}.apk, openbmt-box-<version>-x86_64-linux,
# openbmt-keymaker-<version>.html and COMMIT. The release goes to
# <workdir>/../release-<version>/. The test script uses it.
set -euo pipefail
release_name=release-sign.sh
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/release-lib.sh
. "$here/release-lib.sh"

usage() {
  echo "usage: scripts/release-sign.sh <version> <keydir> [gpg-key]" >&2
  echo "       scripts/release-sign.sh --sign-only <workdir> <version> <keydir> [gpg-key]" >&2
  exit 2
}
sign_only=0
if [ "${1:-}" = --sign-only ]; then
  sign_only=1
  [ $# -ge 4 ] && [ $# -le 5 ] || usage
  workdir_arg=$2; shift 2
else
  [ $# -ge 2 ] && [ $# -le 3 ] || usage
fi
version=$1 keydir_arg=$2 gpgkey=${3:-}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "release-sign.sh: version '$version' is not like 0.1.0" >&2; exit 2; }

top=$(cd "$here" && git rev-parse --show-toplevel)
release_tools
release_check_keydir "$keydir_arg"
keydir=$release_keydir
for app in $release_apps; do
  [ -f "$keydir/openbmt-$app.p12" ] || release_die "refused: $keydir/openbmt-$app.p12 is missing (scripts/release-keys-make.sh makes it)"
done
signer=$(release_apksigner) || release_die "apksigner is needed (Android build-tools; set ANDROID_SDK or APKSIGNER)"
zipalign="$release_bt/zipalign"
[ -x "$zipalign" ] || zipalign=$(command -v zipalign || true)
[ -n "$zipalign" ] || release_die "zipalign is needed"

box_name=openbmt-box-$version-x86_64-linux
kmk_name=openbmt-keymaker-$version.html

if [ $sign_only = 1 ]; then
  [ -d "$workdir_arg" ] || release_die "no directory $workdir_arg"
  work=$(cd "$workdir_arg" && pwd -P)
  dist=$(dirname "$work")
else
  cd "$top"
  [ -z "$(git status --porcelain --untracked-files=all)" ] || release_die "refused: the working tree is not clean; check out a commit and remove changes"
  [ "$(uname -s)-$(uname -m)" = Linux-x86_64 ] || release_die "the Box binary is built for x86_64 Linux; this host is $(uname -s)-$(uname -m)"
  dist=$top/dist
  work=$dist/work-$version
  commit=$(git rev-parse HEAD)
  guard="${OPENBMT_GUARD:-$HOME/.cache/openbmt/guard.sh}"
  [ -x "$guard" ] || release_die "no memory guard at $guard (set OPENBMT_GUARD)"
  rm -rf "$work"
  mkdir -p "$work"
  export SOURCE_DATE_EPOCH
  SOURCE_DATE_EPOCH=$(git log -1 --format=%ct HEAD)
  export OPENBMT_NO_SCCACHE=1
  target=${OPENBMT_RELEASE_TARGET:-$HOME/.cache/openbmt/target/release-sign}
  for app in $release_apps; do
    up=$(printf '%s' "$app" | tr a-z A-Z)
    echo "== $app APK"
    mkdir -p "$work/build-$app"
    env "${up}APK_OUT=$work/build-$app" "${up}APK_CARGO_TARGET=$target/$app" "${up}APK_STOP_AFTER_ALIGN=1" \
      "${up}APK_KEYDIR=$work/build-$app-keys" "$top/apps/$app-android/build.sh" >"$work/build-$app.log" 2>&1 \
      || { tail -n 20 "$work/build-$app.log" >&2; release_die "the $app build failed (log $work/build-$app.log)"; }
    cp "$work/build-$app/aligned.apk" "$work/aligned-$app.apk"
  done
  echo "== Box binary"
  "$guard" env CARGO_TARGET_DIR="$target/box" CARGO_BUILD_JOBS=2 "$top/scripts/box-build.sh" >"$work/build-box.log" 2>&1 \
    || { tail -n 20 "$work/build-box.log" >&2; release_die "the Box build failed (log $work/build-box.log)"; }
  cp "$target/box/release/openbmt-box" "$work/$box_name"
  echo "== key maker page"
  "$top/scripts/keymaker-release.sh" "$version" >"$work/build-keymaker.log" 2>&1 \
    || { tail -n 20 "$work/build-keymaker.log" >&2; release_die "the key maker build failed (log $work/build-keymaker.log)"; }
  cp "$top/tools/keymaker/dist/release/$kmk_name" "$work/$kmk_name"
  printf 'commit %s\n' "$commit" > "$work/COMMIT"
  [ -z "$(git status --porcelain --untracked-files=all)" ] || release_die "the build changed the working tree"
fi

for f in aligned-owner.apk aligned-counter.apk aligned-key.apk "$box_name" "$kmk_name" COMMIT; do
  [ -f "$work/$f" ] || release_die "$work/$f is missing"
done
grep -qE '^commit [0-9a-f]{40}$' "$work/COMMIT" || release_die "COMMIT is not 'commit <40 hex>'"

out=$dist/release-$version
[ ! -e "$out" ] || release_die "refused: $out already exists"
mkdir -p "$out"
finished=0
trap '[ "$finished" = 1 ] || rm -rf "$out"' EXIT

pass_opt() { if [ "$release_test" = 1 ]; then printf '%s\n' "--ks-pass" "file:$keydir/test.pass"; fi; }
for app in $release_apps; do
  mapfile -t po < <(pass_opt)
  echo "== signing $app (apksigner asks for the passphrase of openbmt-$app.p12)"
  "$signer" sign --ks "$keydir/openbmt-$app.p12" --ks-key-alias "openbmt-$app" "${po[@]}" \
    --min-sdk-version "$release_min_sdk" --out "$out/openbmt-$app-$version.apk" "$work/aligned-$app.apk"
  "$signer" verify --min-sdk-version "$release_min_sdk" "$out/openbmt-$app-$version.apk" >/dev/null \
    || release_die "apksigner verify failed for the $app APK"
  "$zipalign" -c -P 16 4 "$out/openbmt-$app-$version.apk" || release_die "the signed $app APK is not aligned"
  rm -f "$out/openbmt-$app-$version.apk.idsig"
done
cp "$work/$box_name" "$work/$kmk_name" "$work/COMMIT" "$out/"
(cd "$out" && LC_ALL=C sha256sum -- $(ls -1 | LC_ALL=C sort) > SHA256SUMS)

gpg_opts=(--armor --detach-sign)
[ "$release_test" = 1 ] && gpg_opts+=(--batch)
[ -z "$gpgkey" ] || gpg_opts+=(--local-user "$gpgkey")
echo "== signing SHA256SUMS (gpg asks for the passphrase of the key)"
(cd "$out" && gpg "${gpg_opts[@]}" SHA256SUMS)
status=$(cd "$out" && gpg --status-fd 1 --verify SHA256SUMS.asc SHA256SUMS 2>/dev/null) || release_die "gpg --verify of SHA256SUMS.asc failed"
fpr=$(printf '%s\n' "$status" | awk '$2 == "VALIDSIG" {print $NF}')
[ -n "$fpr" ] || release_die "gpg gave no valid signature"

echo
echo "release: $out"
cat "$out/SHA256SUMS"
echo "gpg key fingerprint: $fpr"
keys=$(release_keys_file)
fails=0
for app in $release_apps; do
  cert=$(release_cert_sha256 "$out/openbmt-$app-$version.apk")
  echo "$app APK signing certificate SHA-256: $cert"
  want=$(release_keys_value "$keys" "$app") || exit 1
  if [ -z "$want" ]; then echo "$app: no fingerprints are listed yet"
  elif [ "$want" = "$cert" ]; then echo "$app: matches $(basename "$keys")"
  else echo "$app: MISMATCH, $(basename "$keys") lists $want" >&2; fails=1; fi
done
want=$(release_keys_value "$keys" fingerprint) || exit 1
if [ -z "$want" ]; then echo "gpg: no fingerprints are listed yet"
elif [ "$want" = "$(printf '%s' "$fpr" | tr A-F a-f)" ]; then echo "gpg: matches $(basename "$keys")"
else echo "gpg: MISMATCH, $(basename "$keys") lists $want" >&2; fails=1; fi
if [ $fails != 0 ]; then
  rm -rf "$out"
  release_die "refused: a fingerprint does not match $(basename "$keys"); the release was removed"
fi
finished=1
