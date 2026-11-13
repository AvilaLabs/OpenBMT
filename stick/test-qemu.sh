#!/usr/bin/env bash
# SK-40 and SK-41: builds the folder and starts it in QEMU with OVMF in Secure
# Boot mode, Microsoft's keys enrolled and no network device; drives whole runs
# with QEMU's key input and checks the public result against the Rust core.
#
#   stick/test-qemu.sh
#
# Run it under the memory guard. STICK_KEEP=1 keeps the working folder. Needs qemu-system-x86_64, the OVMF files,
# sbattach (sbsigntool), python3, cpio, and the packages of stick/inputs.lock
# (kept between runs in $STICK_DEBS, default ~/.cache/openbmt/stick-debs).
set -euo pipefail
export LC_ALL=C
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(dirname "$here")"
cache="$HOME/.cache/openbmt"
export STICK_DEBS="${STICK_DEBS:-$cache/stick-debs}"
tmp="$(mktemp -d "$cache/tmp/stick-qemu.XXXXXX")"
qemu_pid=""
cleanup() {
    [ -n "$qemu_pid" ] && kill "$qemu_pid" 2>/dev/null || true
    if [ -n "${STICK_KEEP:-}" ]; then echo "kept: $tmp"; else rm -rf "$tmp"; fi
}
trap cleanup EXIT
fail() { echo "FAIL $*" >&2; exit 1; }

for t in qemu-system-x86_64 sbattach python3 cpio gzip; do
    command -v "$t" >/dev/null || fail "$t is needed"
done
code=/usr/share/OVMF/OVMF_CODE_4M.secboot.fd
vars=/usr/share/OVMF/OVMF_VARS_4M.ms.fd
[ -f "$code" ] && [ -f "$vars" ] || fail "OVMF secure boot files are missing"
accel=tcg
[ -w /dev/kvm ] && accel=kvm

# ---- the folders (SK-01 to SK-05) -------------------------------------------
echo "== building the folders"
STICK_FEATURES=serial-echo "$here/build.sh" "$tmp/echo" >/dev/null
# SK-58: the build whose frames come from the serial port (it implies serial-echo).
STICK_FEATURES=serial-frames "$here/build.sh" "$tmp/frames" >/dev/null
"$here/build.sh" "$tmp/release" >/dev/null
"$here/build.sh" "$tmp/release2" >/dev/null
if diff -r "$tmp/release" "$tmp/release2" >/dev/null; then
    echo "PASS SK-05 two builds of one commit are byte-identical: $(sha256sum < "$tmp/release/SHA256SUMS" | cut -d' ' -f1) (SHA256SUMS of SHA256SUMS)"
else
    fail "SK-05 two builds differ"
fi
want_files="EFI/BOOT/BOOTX64.EFI EFI/BOOT/grub.cfg EFI/BOOT/grubx64.efi EFI/ubuntu/grub.cfg SHA256SUMS initrd.img vmlinuz"
got_files="$(cd "$tmp/release" && find . -type f | sed 's|^\./||' | sort | tr '\n' ' ' | sed 's/ $//')"
[ "$got_files" = "$want_files" ] || fail "SK-01 the folder holds: $got_files"
echo "PASS SK-01 the folder holds exactly: $got_files"
(cd "$tmp/release" && sha256sum -c --quiet SHA256SUMS) && echo "PASS SK-05 SHA256SUMS checks"

# SK-40: the serial-echo strings are in the test build and not in the release.
unpack_init() { mkdir -p "$2" && (cd "$2" && gzip -dc "$1/initrd.img" | cpio --quiet -id init); }
unpack_init "$tmp/echo" "$tmp/u-echo"
unpack_init "$tmp/release" "$tmp/u-release"
unpack_init "$tmp/frames" "$tmp/u-frames"
for s in OBM-ECHO /dev/ttyS0; do
    grep -q -a "$s" "$tmp/u-echo/init" || fail "the test build lacks $s"
    if grep -q -a "$s" "$tmp/u-release/init"; then fail "SK-40 the release holds $s"; fi
done
echo "PASS SK-40 the release program holds no serial-echo string; the test build does"
# SK-58: the serial-frames string is in the frames build only.
grep -q -a "OBM-FRAME" "$tmp/u-frames/init" || fail "the frames build lacks OBM-FRAME"
for b in echo release; do
    if grep -q -a "OBM-FRAME" "$tmp/u-$b/init"; then fail "SK-58 the $b build holds OBM-FRAME"; fi
done
echo "PASS SK-58 the release and the serial-echo build hold no serial-frames string; the frames build does"
# SK-10, SK-12: the program is static and holds no network call (strings of the
# libc's network functions are not called by us; the source test greps ours).
if readelf -l "$tmp/u-release/init" | grep -q INTERP; then fail "SK-10 not static"; fi
echo "PASS SK-10 the program is one static file; initrd.img holds:"
(cd "$tmp/release" && gzip -dc initrd.img | cpio --quiet -t | sed 's/^/     /')
# SK-04, SK-51: the camera modules are in initrd.img, apart from the first list's.
list="$(cd "$tmp/release" && gzip -dc initrd.img | cpio --quiet -t)"
for m in $(grep -v '^#' "$here/modules-camera.list"); do
    echo "$list" | grep -q -x "lib/camera-modules/7.0.0-42-generic/$m" || fail "SK-51 initrd.img lacks the camera module $m"
done
for m in $(grep -v '^#' "$here/modules.list"); do
    echo "$list" | grep -q -x "lib/modules/7.0.0-42-generic/$m" || fail "SK-04 initrd.img lacks $m"
done
if echo "$list" | grep '^lib/modules/' | grep -q media; then fail "SK-51 a camera module is in the folder the loader reads at start"; fi
echo "PASS SK-51 the camera modules are in initrd.img under lib/camera-modules, and none is under lib/modules"

# ---- the expected-result tool (the Rust core) -------------------------------
(cd "$repo" && cargo build --locked -q -p openbmt-stick --example expected --example payment >&2)
expected="${CARGO_TARGET_DIR:-$repo/target}/debug/examples/expected"
payment="${CARGO_TARGET_DIR:-$repo/target}/debug/examples/payment"
[ -x "$expected" ] || fail "no $expected"
[ -x "$payment" ] || fail "no $payment"
mkdir -p "$tmp/pay"
"$payment" make "$tmp/pay" | sed 's/^/     payment: /'

run_qemu() { # <folder> <log> <monitor> [extra qemu args...]
    local folder="$1" log="$2" mon="$3"
    shift 3
    cp "$vars" "$tmp/vars.fd"
    : > "$log"
    # A frames run takes its frames from the serial port: the port is a
    # server on a path, logged to the same file the screens are read from.
    local serial=(-serial file:"$log")
    if [ -n "${serial_path:-}" ]; then
        serial=(-chardev "socket,id=ser0,path=$serial_path,server=on,wait=off,logfile=$log" -serial chardev:ser0)
    fi
    timeout 300 qemu-system-x86_64 -machine q35,smm=on -m 512 -display none -no-reboot \
        -accel "$accel" -cpu max \
        -global driver=cfi.pflash01,property=secure,value=on \
        -drive if=pflash,format=raw,unit=0,readonly=on,file="$code" \
        -drive if=pflash,format=raw,unit=1,file="$tmp/vars.fd" \
        -drive file=fat:rw:"$folder",format=raw,if=virtio -nic none \
        "${serial[@]}" -monitor unix:"$mon",server,nowait "$@" &
    qemu_pid=$!
}

wait_qemu() {
    local n=0
    while kill -0 "$qemu_pid" 2>/dev/null; do
        sleep 0.5
        n=$((n + 1))
        [ "$n" -lt 240 ] || fail "QEMU did not power off"
    done
    wait "$qemu_pid" 2>/dev/null || true
    qemu_pid=""
}

drive() { # <scenario> <extra qemu args...>
    local sc="$1" folder="$tmp/echo"
    shift
    echo "== run: $sc"
    local extra=()
    serial_path=""
    case "$sc" in
        sign-*) folder="$tmp/frames"
                serial_path="$tmp/ser-$sc"
                extra=("$payment" "$tmp/pay" "$serial_path") ;;
        camera) extra=("$payment" "$tmp/pay" "") ;;
    esac
    run_qemu "$folder" "$tmp/$sc.log" "$tmp/mon-$sc" "$@"
    python3 -I "$here/qemu-drive.py" "$sc" "$tmp/mon-$sc" "$tmp/$sc.log" "$expected" "$folder/initrd.img" "${extra[@]}" | tee "$tmp/$sc.out"
    wait_qemu
    serial_path=""
    echo "PASS SK-29 the machine powered off by itself"
    grep -a "OBM-ECHO INPUTS" "$tmp/$sc.log" | tr -d '\r' | tail -1 | sed 's/^OBM-ECHO /     /' || true
}

# STICK_ONLY="sign-ur camera" runs just those runs (for work on one of them).
want() { [ -z "${STICK_ONLY:-}" ] || [[ " $STICK_ONLY " == *" $1 "* ]]; }
if want dice; then drive dice; fi
if want mix; then drive mix; fi
# The keyboard of the third run is a USB one on top of the PS/2 one: its
# driver is one of the stick's modules (SK-04).
if want type; then
    drive type -device qemu-xhci -device usb-kbd
    grep -a "OBM-ECHO INPUTS" "$tmp/type.log" | grep -q "USB Keyboard" || fail "SK-04 the USB keyboard driver was not loaded"
    echo "PASS SK-04 the USB keyboard's driver (usbhid) was loaded, the keyboard appeared and typed"
fi

# ---- SK-50 to SK-58: Sign a payment -----------------------------------------
# The camera modules load only after the choice; QEMU has no camera, so the
# serial-echo build ends on the no-camera screen.
if want camera; then drive camera; fi
# A whole payment signed from frames fed through the serial port, in both forms.
if want sign-ur; then drive sign-ur; fi
if want sign-bbqr; then drive sign-bbqr; fi
if want sign-refuse; then drive sign-refuse; fi

# ---- SK-41: refused when unsigned ------------------------------------------
echo "== run: unsigned"
cp -r "$tmp/echo" "$tmp/unsigned"
chmod -R u+w "$tmp/unsigned"
sbattach --remove "$tmp/unsigned/EFI/BOOT/BOOTX64.EFI" >/dev/null
run_qemu "$tmp/unsigned" "$tmp/unsigned.log" "$tmp/mon-unsigned"
sleep 45
kill "$qemu_pid" 2>/dev/null || true
wait "$qemu_pid" 2>/dev/null || true
qemu_pid=""
if grep -a -q "OBM-ECHO" "$tmp/unsigned.log"; then
    fail "SK-41 the copy without a signature started"
fi
echo "PASS SK-41 the copy whose BOOTX64.EFI has no signature did not start; serial log:"
tr -d '\r' < "$tmp/unsigned.log" | sed 's/\x1b\[[0-9;?=]*[a-zA-Z]//g' | grep -a . | head -5 | sed 's/^/     /'
echo "== all passed"
