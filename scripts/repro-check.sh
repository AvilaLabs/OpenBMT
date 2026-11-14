#!/usr/bin/env bash
# Reproducibility check: builds the release artifacts twice from two separate
# clean copies of one commit, at two different absolute paths, each with its
# own cargo target directory and its own TMPDIR, and compares sha256 sums.
#
# Artifacts compared: for each of the Key, Owner and Counter apps the signed
# APK, the unsigned aligned APK (the stage before signing), classes.dex, and
# every stripped native library in the APK; and the Box binary
# (scripts/box-build.sh: cargo build --release -p openbmt-box, as
# packaging/box/Dockerfile does). The APKs are signed with one local RSA test
# key per app, made by the first build and kept in $REPRO_DIR/keys (not the
# usual test keys: an RSA PKCS#1 v1.5 signature is deterministic, an ECDSA one
# is not, so an EC key can never give a matching signed APK). A signed-APK
# mismatch with a matching aligned stage would point at the signer.
#
# Usage: scripts/repro-check.sh [commit]      (default: HEAD of this checkout)
#   REPRO_DIR    work directory (default: ~/.cache/openbmt/repro); the two
#                copies are worktrees in $REPRO_DIR/a and $REPRO_DIR/b
#   REPRO_KEEP   set to 1 to keep the work directory after the run
#   REPRO_ONLY   space-separated subset of: key owner counter box
#
# Needs the environment the APK build scripts need (JAVA_HOME, KOTLIN_HOME).
# Every build runs under the memory guard, one at a time. Exits 0 when every
# artifact matches, 1 when any differs.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cache="$HOME/.cache/openbmt"
guard="${OPENBMT_GUARD:-$cache/guard.sh}"
base="${REPRO_DIR:-$cache/repro}"
only="${REPRO_ONLY:-key owner counter box}"
min_free_gb=60

die() { printf 'repro-check.sh: %s\n' "$1" >&2; exit 2; }

commit="$(git -C "$here" rev-parse --verify "${1:-HEAD}^{commit}")" || die "no such commit"
[ -x "$guard" ] || die "no memory guard at $guard"
export OPENBMT_NO_SCCACHE=1
export JAVA_HOME="${JAVA_HOME:-$HOME/Android/jdk21}"
export KOTLIN_HOME="${KOTLIN_HOME:-$cache/tools/kotlinc}"

cleanup() {
    [ "${REPRO_KEEP:-0}" = 1 ] && return
    for s in a b; do
        [ -d "$base/$s/.git" ] || [ -f "$base/$s/.git" ] && git -C "$here" worktree remove --force "$base/$s" 2>/dev/null
    done
    rm -rf "$base"
    git -C "$here" worktree prune
}
trap cleanup EXIT

rm -rf "$base"
mkdir -p "$base"
for s in a b; do
    git -C "$here" worktree add --detach "$base/$s" "$commit" >/dev/null 2>&1 || die "could not make the copy at $base/$s"
    mkdir -p "$base/$s-tmp" "$base/$s-target" "$base/$s-out"
done
epoch="$(git -C "$here" log -1 --format=%ct "$commit")"
echo "commit $commit (commit time $epoch)"

free_gb() { df -BG --output=avail "$HOME" | tail -n 1 | tr -dc '0-9'; }

build_side() { # a|b
    local s="$1" src="$base/$1" t="$base/$1-target" tmp="$base/$1-tmp" out="$base/$1-out"
    local free
    free="$(free_gb)"
    [ "$free" -ge "$min_free_gb" ] || die "only $free GB free under $HOME (need $min_free_gb); stopping"
    echo "== copy $s: $src (free: $free GB)"
    local app up
    for app in key owner counter; do
        case " $only " in *" $app "*) ;; *) continue ;; esac
        up="$(printf '%s' "$app" | tr a-z A-Z)"
        echo "-- $app apk ($s)"
        "$guard" env TMPDIR="$tmp" \
            "${up}APK_OUT=$out/$app" "${up}APK_CARGO_TARGET=$t" "${up}APK_KEYDIR=$base/keys/$app" \
            "$src/apps/$app-android/build.sh" >"$out/$app.log" 2>&1 \
            || { tail -n 20 "$out/$app.log" >&2; die "the $app build failed in copy $s (log $out/$app.log)"; }
    done
    case " $only " in *" box "*)
        echo "-- box ($s)"
        "$guard" env CARGO_TARGET_DIR="$t" TMPDIR="$tmp" CARGO_BUILD_JOBS=2 \
            "$src/scripts/box-build.sh" >"$out/box.log" 2>&1 \
            || { tail -n 20 "$out/box.log" >&2; die "the box build failed in copy $s"; } ;;
    esac
}

time_start=$SECONDS
build_side a
t_a=$((SECONDS - time_start))
build_side b
t_b=$((SECONDS - time_start - t_a))
echo "build time: copy a ${t_a}s, copy b ${t_b}s"

# ---- compare --------------------------------------------------------------------------

fails=0
sum() { [ -f "$1" ] && sha256sum "$1" | cut -d' ' -f1 || echo missing; }
cmp_one() { # label path-below-out-dir
    local label="$1" rel="$2" ha hb
    ha="$(sum "$base/a-out/$rel")"
    hb="$(sum "$base/b-out/$rel")"
    if [ "$ha" = "$hb" ] && [ "$ha" != missing ]; then
        printf 'MATCH     %-52s %s\n' "$label" "$ha"
    else
        printf 'DIFFERENT %-52s a=%s b=%s\n' "$label" "$ha" "$hb"
        fails=$((fails + 1))
    fi
}

echo
echo "== comparison"
for app in key owner counter; do
    case " $only " in *" $app "*) ;; *) continue ;; esac
    cmp_one "$app signed apk" "$app/openbmt-$app.apk"
    cmp_one "$app aligned apk (unsigned)" "$app/aligned.apk"
    cmp_one "$app classes.dex" "$app/dex/classes.dex"
    for f in "$base"/a-out/$app/stage/lib/*/*.so; do
        rel="${f#"$base/a-out/"}"
        cmp_one "$app ${rel#"$app/stage/lib/"}" "$rel"
    done
done
case " $only " in *" box "*)
    ha="$(sum "$base/a-target/release/openbmt-box")"
    hb="$(sum "$base/b-target/release/openbmt-box")"
    if [ "$ha" = "$hb" ] && [ "$ha" != missing ]; then
        printf 'MATCH     %-52s %s\n' "box binary" "$ha"
    else
        printf 'DIFFERENT %-52s a=%s b=%s\n' "box binary" "$ha" "$hb"
        fails=$((fails + 1))
    fi ;;
esac

if [ "$fails" -eq 0 ]; then
    echo "result: every artifact matches"
    exit 0
fi
echo "result: $fails artifact(s) differ"
[ "${REPRO_KEEP:-0}" = 1 ] || echo "(run with REPRO_KEEP=1 to keep the copies and inspect the differences)"
exit 1
