#!/usr/bin/env bash
# Builds the stick folder (docs/STICK-RULES.md, SK-01 to SK-05).
#
#   stick/build.sh <new-folder>
#
# It downloads exactly the packages of stick/inputs.lock into a fresh empty
# directory under ~/.cache/openbmt/stick-build, refuses any whose SHA-256
# differs, takes the distribution's files from them unchanged, builds the
# program of crates/openbmt-stick (static), packs initrd.img and writes the
# folder with its SHA256SUMS. Two runs on one commit with one inputs.lock give
# byte-identical files.
#
# Environment:
#   STICK_FEATURES  cargo features of the program ("serial-echo" is for
#                   stick/test-qemu.sh only, never for a release)
#   STICK_DEBS      a directory that keeps the packages between runs (each is
#                   checked against inputs.lock every time; a missing one is
#                   downloaded into it); default: a fresh directory each run
#   STICK_BINARY    use this program file instead of building one
#   SOURCE_DATE_EPOCH  the time stamped on every file of initrd.img; default
#                   the commit time
set -euo pipefail
export LC_ALL=C TZ=UTC
umask 022

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(dirname "$here")"
out="${1:?usage: stick/build.sh <new-folder>}"
target=x86_64-unknown-linux-gnu

fail() { echo "build.sh: $*" >&2; exit 1; }

if [ -e "$out" ] && [ -n "$(ls -A "$out" 2>/dev/null || echo x)" ]; then
    fail "$out exists and is not empty"
fi
mkdir -p "$out"
out="$(cd "$out" && pwd)"

: "${SOURCE_DATE_EPOCH:=$(git -C "$repo" log -1 --format=%ct)}"
export SOURCE_DATE_EPOCH
commit="$(git -C "$repo" rev-parse --short=12 HEAD)"

# SK-02: never under /tmp; a fresh empty directory each run.
base="$HOME/.cache/openbmt/stick-build"
mkdir -p "$base"
work="$(mktemp -d "$base/run.XXXXXX")"
trap 'rm -rf "$work"' EXIT

# ---- the packages (SK-02) ---------------------------------------------------
debs="${STICK_DEBS:-$work/debs}"
mkdir -p "$debs"
declare -A deb_of
while read -r pkg ver sha _; do
    case "$pkg" in ''|'#'*) continue ;; esac
    files=("$debs/${pkg}_"*.deb)
    if [ ! -f "${files[0]}" ]; then
        (cd "$debs" && apt-get download "$pkg=$ver" >/dev/null)
        files=("$debs/${pkg}_"*.deb)
    fi
    [ "${#files[@]}" -eq 1 ] && [ -f "${files[0]}" ] || fail "$pkg: expected one package file in $debs"
    got="$(sha256sum "${files[0]}" | cut -d' ' -f1)"
    [ "$got" = "$sha" ] || fail "$pkg $ver: SHA-256 $got differs from inputs.lock"
    deb_of[$pkg]="${files[0]}"
done < "$here/inputs.lock"
for p in shim-signed grub-efi-amd64-signed linux-image-7.0.0-42-generic linux-modules-7.0.0-42-generic; do
    [ -n "${deb_of[$p]:-}" ] || fail "inputs.lock has no $p"
done
kver=7.0.0-42-generic

# ---- SK-04: the modules list refuses what a stick may not carry ------------
modules=()
while read -r line; do
    case "$line" in ''|'#'*) continue ;; esac
    case "$line" in
        kernel/drivers/net/*|kernel/net/*|kernel/drivers/bluetooth/*|kernel/drivers/usb/storage/*|\
        kernel/drivers/scsi/*|kernel/drivers/ata/*|kernel/drivers/nvme/*|kernel/fs/*|kernel/sound/*|\
        kernel/drivers/media/*)
            fail "modules.list: $line is a module a stick may not carry (SK-04)" ;;
    esac
    case "$line" in */../*|/*) fail "modules.list: $line" ;; esac
    modules+=("$line")
done < "$here/modules.list"
[ "${#modules[@]}" -gt 0 ] || fail "modules.list is empty"

# ---- unpack what is needed (unchanged) --------------------------------------
x="$work/x"
mkdir -p "$x"
for p in shim-signed grub-efi-amd64-signed linux-image-7.0.0-42-generic; do
    dpkg-deb -x "${deb_of[$p]}" "$x/$p"
done
modx="$x/modules"
mkdir -p "$modx"
want=()
for m in "${modules[@]}"; do want+=("./usr/lib/modules/$kver/$m"); done
dpkg-deb --fsys-tarfile "${deb_of[linux-modules-7.0.0-42-generic]}" | tar -x -C "$modx" -f - "${want[@]}"

shim="$x/shim-signed/usr/lib/shim/shimx64.efi.signed.latest"
grub="$x/grub-efi-amd64-signed/usr/lib/grub/x86_64-efi-signed/grubx64.efi.signed"
kernel="$x/linux-image-7.0.0-42-generic/boot/vmlinuz-$kver"
for f in "$shim" "$grub" "$kernel"; do [ -f "$f" ] || fail "missing $f in its package"; done

# ---- the program (SK-10) ----------------------------------------------------
if [ -n "${STICK_BINARY:-}" ]; then
    prog="$STICK_BINARY"
else
    flags="-C target-feature=+crt-static -C strip=symbols"
    flags="$flags --remap-path-prefix=$repo=/src --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$HOME/.rustup=/rustup"
    (cd "$repo" && OPENBMT_STICK_COMMIT="$commit" RUSTFLAGS="$flags" \
        cargo build --release --locked --target "$target" -p openbmt-stick \
        ${STICK_FEATURES:+--features "$STICK_FEATURES"} >&2)
    prog="${CARGO_TARGET_DIR:-$repo/target}/$target/release/openbmt-stick"
fi
[ -x "$prog" ] || fail "no program at $prog"
if readelf -l "$prog" | grep -q INTERP; then fail "the program is not static"; fi

# ---- initrd.img (SK-03, SK-05) ----------------------------------------------
st="$work/initrd"
mkdir -p "$st/proc" "$st/sys" "$st/dev"
install -m 0755 "$prog" "$st/init"
for m in "${modules[@]}"; do
    install -D -m 0644 "$modx/usr/lib/modules/$kver/$m" "$st/lib/modules/$kver/$m"
done
find "$st" -type d -exec chmod 0755 {} +
find "$st" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
(cd "$st" && find . -mindepth 1 | sort | cpio --quiet -o -H newc --reproducible --owner=0:0 | gzip -n -9) > "$work/initrd.img"

# ---- the folder (SK-01, SK-03) ----------------------------------------------
cmdline="$(cat "$here/cmdline")"
cat > "$work/grub.cfg" <<GRUB
set timeout=0
search --no-floppy --file --set=root /initrd.img
linux /vmlinuz $cmdline
initrd /initrd.img
boot
GRUB
mkdir -p "$out/EFI/BOOT" "$out/EFI/ubuntu"
install -m 0644 "$shim" "$out/EFI/BOOT/BOOTX64.EFI"
install -m 0644 "$grub" "$out/EFI/BOOT/grubx64.efi"
install -m 0644 "$work/grub.cfg" "$out/EFI/BOOT/grub.cfg"
install -m 0644 "$work/grub.cfg" "$out/EFI/ubuntu/grub.cfg"
install -m 0644 "$kernel" "$out/vmlinuz"
install -m 0644 "$work/initrd.img" "$out/initrd.img"
(cd "$out" && find . -type f ! -name SHA256SUMS | sort | sed 's|^\./||' | xargs sha256sum > SHA256SUMS)
echo "stick folder: $out"
cat "$out/SHA256SUMS"
