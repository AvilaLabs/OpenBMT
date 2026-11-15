# Shared by scripts/release-keys-make.sh, release-sign.sh and release-verify.sh (sourced, not run).
#
# Test mode: OPENBMT_RELEASE_TEST=1 lets the tools read a passphrase from
# <keydir>/test.pass and run gpg without a prompt. It is refused unless the key
# directory is under ~/.cache/openbmt/tmp/release-test/. Without it nothing here
# takes a passphrase from an argument, a variable or a file.

release_die() { printf '%s: %s\n' "${release_name:-release}" "$1" >&2; exit 1; }

release_apps="owner counter key"
release_java_home="${JAVA_HOME:-$HOME/Android/jdk21}"
release_sdk="${ANDROID_SDK:-$HOME/Android/Sdk}"
release_bt="$release_sdk/build-tools/36.0.0"
release_min_sdk=29
release_test_root="$HOME/.cache/openbmt/tmp/release-test"

# release_check_keydir <keydir>: sets release_keydir (absolute) and release_test (0 or 1).
release_check_keydir() {
    local top real
    [ -n "${1:-}" ] || release_die "no key directory given"
    mkdir -p "$1" || release_die "cannot make $1"
    real=$(cd "$1" && pwd -P) || release_die "cannot enter $1"
    top=$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel 2>/dev/null || true)
    if [ -n "$top" ]; then
        top=$(cd "$top" && pwd -P)
        case "$real/" in
            "$top"/*) release_die "refused: $real is inside the repository ($top); keep the keys outside it" ;;
        esac
    fi
    release_keydir=$real
    release_test=0
    if [ "${OPENBMT_RELEASE_TEST:-}" = 1 ]; then
        local root
        mkdir -p "$release_test_root"
        root=$(cd "$release_test_root" && pwd -P)
        case "$real/" in
            "$root"/?*) ;;
            *) release_die "refused: OPENBMT_RELEASE_TEST=1 only works for a key directory under $release_test_root" ;;
        esac
        [ -f "$real/test.pass" ] || release_die "test mode needs $real/test.pass"
        release_test=1
    fi
}

release_tools() {
    export PATH="$release_java_home/bin:$PATH"
    command -v keytool >/dev/null || release_die "keytool is needed (a JDK; set JAVA_HOME)"
    command -v gpg >/dev/null || release_die "gpg is needed"
}

release_apksigner() {
    if [ -n "${APKSIGNER:-}" ]; then printf '%s\n' "$APKSIGNER"
    elif [ -x "$release_bt/apksigner" ]; then printf '%s\n' "$release_bt/apksigner"
    elif command -v apksigner >/dev/null; then command -v apksigner
    else return 1; fi
}

# release_cert_sha256 <apk>: prints the signing certificate's SHA-256 (64 lowercase hex), one signer only.
release_cert_sha256() {
    local signer out
    signer=$(release_apksigner) || release_die "apksigner is needed (Android build-tools; set ANDROID_SDK or APKSIGNER)"
    out=$("$signer" verify --print-certs "$1" 2>&1) || release_die "apksigner does not accept $1"
    out=$(printf '%s\n' "$out" | sed -n 's/^Signer #[0-9]* certificate SHA-256 digest: \([0-9a-fA-F]\{64\}\)$/\1/p')
    [ "$(printf '%s\n' "$out" | grep -c .)" = 1 ] || release_die "$1 has no single signing certificate"
    printf '%s\n' "$out" | tr 'A-F' 'a-f'
}

# release_keys_value <file> <label>: prints the hex listed for the label, or nothing when none is listed.
# A line is "<label>: <hex>" or "<label> <hex>" (hex: 40 for fingerprint, 64 for apps, either case, spaces
# inside the hex not allowed). A line starting with the label that holds anything else than hex or
# "(none yet)", or two lines for one label, stops the script.
release_keys_value() {
    local file=$1 label=$2 len lines n line val
    [ -f "$file" ] || release_die "no $file"
    if [ "$label" = fingerprint ]; then len=40; else len=64; fi
    lines=$(grep -E "^${label}([: ]|\$)" "$file" || true)
    n=$(printf '%s' "$lines" | grep -c . || true)
    [ "$n" -le 1 ] || release_die "$file lists '$label' $n times; exactly one line is allowed"
    [ "$n" = 1 ] || { return 0; }
    line=$lines
    val=$(printf '%s\n' "$line" | sed -E "s/^${label}:?[[:space:]]+//; s/[[:space:]]+\$//")
    case "$val" in
        "(none yet)") return 0 ;;
    esac
    printf '%s\n' "$val" | grep -qE "^[0-9a-fA-F]{$len}\$" || release_die "the '$label' line in $file is not $len hex digits: $line"
    printf '%s\n' "$val" | tr 'A-F' 'a-f'
}

# release_keys_file: the list of fingerprints (the repository's own; test mode may name another).
release_keys_file() {
    if [ "${OPENBMT_RELEASE_TEST:-}" = 1 ] && [ -n "${OPENBMT_RELEASE_KEYS_FILE:-}" ]; then
        printf '%s\n' "$OPENBMT_RELEASE_KEYS_FILE"
    else
        printf '%s\n' "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/docs/RELEASE-KEYS.md"
    fi
}
