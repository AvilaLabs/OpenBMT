#!/usr/bin/env python3
"""Drives one run of the stick in QEMU through the monitor's `sendkey`, reading
the screens from the serial log (stick/test-qemu.sh, SK-40).

  python3 -I stick/qemu-drive.py <scenario> <monitor-socket> <serial-log> <expected-binary> <initrd.img> [<payment-binary> <payment-dir> <serial-socket>]

Scenarios: dice (dice alone, SK-22), mix (the computer's bytes and rolls,
SK-20/21, with a wrong answer first), type (check my words, SK-28), and with
the last three arguments: sign-ur and sign-bbqr (a whole payment signed from
frames fed through the serial port, SK-50 to SK-58), sign-refuse (a payment
of another wallet), camera (the camera modules load only after the choice;
no camera, SK-51, SK-52).
Every check prints a line starting PASS; any failure exits 1.
"""
import hashlib
import os
import re
import socket
import subprocess
import sys
import time

TEST_SEED = "primary fetch primary fetch primary fetch primary fetch primary fetch primary fever".split()
# The reference's 50 triplets (docs/KEYMAKER-RULES.md KM-30, KM-31).
TRIPLETS = ("136 235 425 512 124 432 523 456 456 652 532 431 651 643 314 361 145 162 145 265 365 641 143 532 "
            "531 541 146 614 461 364 126 324 256 216 214 231 126 325 524 541 265 326 436 416 634 631 461 245 356 352")
DICE_WORDS = "dance summer quarter guitar magic arm response novel toe service light enough"

scenario, mon_path, log_path, expected_bin, initrd = sys.argv[1:6]
pay_bin, pay_dir, ser_path = (sys.argv[6:9] + [None, None, None])[:3]

# The screens' words, read from the program's own source (SK-15), so the
# checks follow the words as they are written. Placeholders (WORDS_NEEDED_...)
# are read under their full name.
_words_rs = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "openbmt-stick", "src", "words.rs")
W = {}
for m in re.finditer(r'pub const ((?:WORDS_NEEDED_)?SK_\w+): &str =\s*"((?:[^"\\]|\\.)*)";', open(_words_rs).read()):
    W[m.group(1)] = m.group(2).replace('\\"', '"')


def fail(msg):
    print("FAIL", scenario, msg, flush=True)
    sys.exit(1)


def ok(msg):
    print("PASS", scenario, msg, flush=True)


# ---- the monitor ------------------------------------------------------------
deadline = time.time() + 30
mon = socket.socket(socket.AF_UNIX)
while True:
    try:
        mon.connect(mon_path)
        break
    except OSError:
        if time.time() > deadline:
            fail("no monitor socket")
        time.sleep(0.2)
mon.settimeout(5)
try:
    mon.recv(4096)
except OSError:
    pass


def sendkey(name):
    mon.sendall(("sendkey " + name + "\n").encode())
    time.sleep(0.02)
    try:
        mon.recv(4096)
    except OSError:
        pass


def shot(name, least):
    """SK-14: the frame buffer shows the screen: the machine's own picture of
    it has at least `least` white pixels (the text, the QR paper)."""
    path = "%s.%s.ppm" % (log_path, name)
    mon.sendall(("screendump %s\n" % path).encode())
    time.sleep(1.0)
    try:
        mon.recv(4096)
    except OSError:
        pass
    data = open(path, "rb").read()
    head = data.split(b"\n", 3)
    w, h = (int(v) for v in head[1].split())
    white = 0
    pix = head[3]
    for i in range(0, len(pix), 3):
        if pix[i] == 255 and pix[i + 1] == 255 and pix[i + 2] == 255:
            white += 1
    if white < least:
        fail("the %s picture has only %d white pixels" % (name, white))
    ok("SK-14 the display (%d x %d) shows the %s screen: %d white pixels" % (w, h, name, white))


# ---- the screens ------------------------------------------------------------
def screens():
    try:
        text = open(log_path, "rb").read().decode("latin-1")
    except OSError:
        return []
    out, cur = [], None
    for raw in text.replace("\r", "").split("\n"):
        if raw.startswith("OBM-ECHO BEGIN "):
            cur = {"id": raw[len("OBM-ECHO BEGIN "):], "lines": [], "qr": []}
        elif raw == "OBM-ECHO END" and cur is not None:
            out.append(cur)
            cur = None
        elif cur is not None and raw.startswith("OBM-ECHO QRROW "):
            cur["qr"].append(raw[len("OBM-ECHO QRROW "):])
        elif cur is not None and raw.startswith("OBM-ECHO "):
            cur["lines"].append(raw[len("OBM-ECHO "):])
    return out


def wait_screens(n, timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        s = screens()
        if len(s) > n:
            return s
        time.sleep(0.05)
    fail("no new screen after %d (last log lines: %r)" % (n, open(log_path, "rb").read()[-300:]))


def press(name):
    n = len(screens())
    sendkey(name)
    return wait_screens(n)[-1]


def type_text(s):
    last = None
    for c in s:
        last = press(c)
    return last


def field(screen, label):
    for line in screen["lines"]:
        if line.startswith(label + " "):
            return line[len(label) + 1:]
    fail("no %s on screen %s: %r" % (label, screen["id"], screen["lines"]))


def expected(*args):
    out = subprocess.run([expected_bin, *args], check=True, capture_output=True, text=True).stdout
    d, rows = {}, {}
    for line in out.splitlines():
        k, _, v = line.partition(" ")
        if k.endswith("row"):
            rows.setdefault(k, []).append(v)
        else:
            d[k] = v
    d.update(rows)
    return d


# ---- the run ----------------------------------------------------------------
if scenario == "type":
    # A USB keyboard enumerates some seconds after the program starts; keys
    # sent before it exists are lost in QEMU. The program echoes its input
    # devices whenever a new one opens.
    end = time.time() + 90
    while "USB Keyboard" not in open(log_path, "rb").read().decode("latin-1"):
        if time.time() > end:
            fail("the USB keyboard never appeared")
        time.sleep(0.2)
    ok("SK-04 the USB keyboard appeared after the stick's modules loaded")

first = wait_screens(0, 120)[0]
if first["id"] != "start":
    fail("first screen is " + first["id"])
if field(first, W["SK_06_SECURE_BOOT"]) != W["SK_06_ON"]:
    fail("SecureBoot is not on: %r" % first["lines"])
ok("SK-06 first screen: SecureBoot " + field(first, W["SK_06_SECURE_BOOT"]))
if field(first, W["SK_06_LOCKDOWN"]) != "integrity":
    fail("lockdown: %r" % first["lines"])
ok("SK-06 first screen: lockdown integrity")
nets = field(first, W["SK_12_INTERFACES"])
if nets != "lo":
    fail("interfaces: " + nets)
ok("SK-12 first screen lists the interfaces: " + nets)
want = hashlib.sha256(open(initrd, "rb").read()).hexdigest()
if field(first, W["SK_31_INITRD_HASH"]) != want:
    fail("initrd hash %s != %s" % (field(first, W["SK_31_INITRD_HASH"]), want))
ok("SK-31 initrd.img SHA-256 as the kernel loaded it equals the file: " + want)
ok("SK-31 version: " + field(first, W["SK_31_VERSION"]))
shot("start", 500)


def read_words():
    """Pages through the three words screens; returns the 12 words, standing
    on the check screen."""
    words = []
    s = screens()[-1]
    for page in range(3):
        if s["id"] != "words":
            fail("expected a words screen, got " + s["id"])
        got = [l.split(" ", 1) for l in s["lines"] if l.split(" ", 1)[0].isdigit()]
        if [int(n) for n, _ in got] != [page * 4 + i + 1 for i in range(4)]:
            fail("numbering on page %d: %r" % (page, got))
        words += [w for _, w in got]
        s = press("ret")
    if s["id"] != "check":
        fail("expected the check screen, got " + s["id"])
    return words


def answer_check(words, wrong_first=False):
    s = screens()[-1]
    asked = []
    for i in range(3):
        pos = int(field(s, W["SK_25_ASK"]))
        asked.append(pos)
        word = "zzzz" if (wrong_first and i == 0) else words[pos - 1][:4]
        type_text(word)
        s = press("ret")
    return asked, s


def check_result(s, words, exp):
    if s["id"] != "result":
        fail("expected the result screen, got " + s["id"] + " " + repr(s["lines"]))
    for k, label in (("fingerprint", W["SK_26_FINGERPRINT"]),
                     ("address", W["SK_26_ADDRESS"]),
                     ("descriptor", W["SK_26_DESCRIPTOR"])):
        if field(s, label) != exp[k]:
            fail("%s: screen %s, core %s" % (k, field(s, label), exp[k]))
    ok("SK-26 fingerprint %s, first address %s" % (exp["fingerprint"], exp["address"]))
    ok("SK-26 descriptor %s" % exp["descriptor"])
    q = press("ret")
    shot("qr", 5000)
    if q["id"] != "result-qr" or q["qr"] != exp["descriptorqrrow"]:
        fail("the descriptor QR differs from the core's")
    ok("SK-26 QR code of the descriptor equals the core's (%d x %d)" % (len(q["qr"]), len(q["qr"])))
    sq = press("s")
    if sq["id"] != "seedqr" or sq["qr"] != exp["seedqrrow"]:
        fail("the SeedQR differs from the core's")
    ok("SK-27 Compact SeedQR equals the core's (%d x %d)" % (len(sq["qr"]), len(sq["qr"])))
    for sc in (s, q, sq):
        if not any(W["SK_29_TURN_OFF"] in l for l in sc["lines"]):
            fail("no Turn off on " + sc["id"])
    ok("SK-29 every screen after the words has Turn off")


if scenario == "dice":
    press("2")
    rolls = TRIPLETS.replace(" ", "")
    s = type_text(rolls)
    if field(s, W["SK_22_VALUES"]) != "50/50":
        fail("values: " + field(s, W["SK_22_VALUES"]))
    ok("SK-22 150 rolls keep 50 values")
    exp = expected("dice", rolls)
    s = press("ret")
    words = read_words()
    if " ".join(words) != DICE_WORDS or exp["words"] != DICE_WORDS:
        fail("words %r" % words)
    ok("SK-22 dice alone gives the Key Maker page's words: " + " ".join(words))
    asked, s = answer_check(words)
    ok("SK-25 positions asked %r, right answers go on" % asked)
    check_result(s, words, exp)
elif scenario == "mix":
    press("1")
    type_text("1234")
    s = press("backspace")
    if field(s, W["SK_21_ROLLS"]) != "3":
        fail("undo: " + field(s, W["SK_21_ROLLS"]))
    ok("SK-21 undo removes the last roll")
    press("ret")
    words = read_words()
    asked, s = answer_check(words, wrong_first=True)
    if s["id"] != "words" or field(s, W["SK_25_WRONG"]) != str(asked[0]):
        fail("a wrong answer did not show the words again: %r" % s["lines"])
    ok("SK-25 a wrong answer shows the words again, naming position %d" % asked[0])
    again = read_words()
    if again != words:
        fail("the words changed")
    asked2, s = answer_check(words)
    if asked2 != asked:
        fail("the positions changed")
    ok("SK-25 the same three positions, right answers go on")
    check_result(s, words, expected("words", *words))
    print("WORDS-MADE-FROM-THE-KERNEL-BYTES-AND-ROLLS", " ".join(words), flush=True)
elif scenario == "type":
    press("3")
    for w in TEST_SEED[:11]:
        type_text(w)
        s = press("ret")
    type_text("abandon")  # a word of the list, but not with this checksum
    s = press("ret")
    if not any(W["SK_28_BAD_CHECKSUM"] in l for l in s["lines"]):
        fail("a wrong 12th word was not refused: %r" % s["lines"])
    ok("SK-28 a wrong checksum is refused")
    for _ in range(7):
        press("backspace")
    type_text("fever")
    s = press("ret")
    exp = expected("words", *TEST_SEED)
    if exp["fingerprint"] != "818bc9b6":
        fail("the core's fingerprint for the test seed")
    check_result(s, TEST_SEED, exp)
elif scenario in ("sign-ur", "sign-bbqr", "sign-refuse", "camera"):
    CAMERA_MODULES = ["mc", "videodev", "videobuf2_common", "videobuf2_memops", "videobuf2_v4l2",
                      "videobuf2_vmalloc", "uvc", "uvcvideo"]

    def modules(when):
        text = open(log_path, "rb").read().decode("latin-1").replace("\r", "")
        for line in text.split("\n"):
            if line.startswith("OBM-ECHO MODULES " + when + " ") or line == "OBM-ECHO MODULES " + when:
                return line.split()[3:]
        return None

    def wait_modules(when, timeout=60):
        end = time.time() + timeout
        while time.time() < end:
            m = modules(when)
            if m is not None:
                return m
            time.sleep(0.1)
        fail("no module list '%s' on the serial port" % when)

    # SK-51, SK-58: the camera modules are not loaded before the choice.
    if any(m in wait_modules("start") for m in CAMERA_MODULES):
        fail("a camera module was loaded before the choice: %r" % modules("start"))
    ok("SK-51 before the choice no camera module is loaded (%d modules: %s)" % (len(modules("start")), " ".join(modules("start"))))
    if not any(W["WORDS_NEEDED_SK_50_MENU_SIGN"] in l and l.lstrip(" >").startswith("4 ") for l in first["lines"]):
        fail("the first screen has no fourth choice: %r" % first["lines"])
    ok("SK-50 the first screen has a fourth choice: Sign a payment")
    s = press("4")
    if s["id"] != "sign-scan":
        fail("the fourth choice opened " + s["id"])
    loaded = wait_modules("camera")
    missing = [m for m in CAMERA_MODULES if m not in loaded]
    if missing:
        fail("camera modules not loaded after the choice: %r (have %r)" % (missing, loaded))
    ok("SK-51 after the choice the camera modules are loaded: " + " ".join(m for m in CAMERA_MODULES))
    shot("scan", 300)

    if scenario == "camera":
        # No camera in QEMU: the screen says so after the set time (15 s).
        end = time.time() + 120
        while time.time() < end:
            cur = screens()[-1]
            if cur["id"] == "sign-nocamera":
                break
            time.sleep(0.5)
        else:
            fail("the no-camera screen never came: %r" % screens()[-1]["lines"])
        if W["WORDS_NEEDED_SK_52_NO_CAMERA"] not in cur["lines"]:
            fail("no-camera words: %r" % cur["lines"])
        ok("SK-52 no camera found: the screen says so")
        shot("nocamera", 300)
        if not any(W["SK_29_TURN_OFF"] in l for l in cur["lines"]):
            fail("no Turn off on the no-camera screen")
        ok("SK-52 the no-camera screen has Turn off")
    else:
        import random
        form = "ur" if scenario != "sign-bbqr" else "bbqr"
        if scenario == "sign-refuse":
            form = "bbqr"
        frames = [l for l in open(os.path.join(pay_dir, form + ".frames")).read().split("\n") if l]
        ser = socket.socket(socket.AF_UNIX)
        end = time.time() + 30
        while True:
            try:
                ser.connect(ser_path)
                break
            except OSError:
                if time.time() > end:
                    fail("no serial socket")
                time.sleep(0.2)
        # The port's output (the screens' echo) is logged by QEMU itself; it
        # must still be read here, or the guest's writes stall when the
        # socket's buffer is full.
        import threading

        def drain():
            try:
                while ser.recv(65536):
                    pass
            except OSError:
                pass

        threading.Thread(target=drain, daemon=True).start()
        rnd = random.Random(5)
        order = frames[:]
        rnd.shuffle(order)
        # Repeats, and a QR text of something else between the parts.
        feed = []
        for f in order:
            feed += [f, "https://example.invalid/not-a-payment", f] if rnd.random() < 0.3 else [f]
        sent = 0
        total_seen = None
        for f in feed:
            ser.sendall(("OBM-FRAME " + f + "\n").encode())
            sent += 1
            time.sleep(0.25)
            cur = screens()[-1]
            if cur["id"] != "sign-scan":
                break
            for l in cur["lines"]:
                if l.startswith(W["WORDS_NEEDED_SK_52_PARTS"] + " "):
                    total_seen = l.split()[-1]
        cur = screens()[-1]
        if cur["id"] != "type":
            fail("the frames did not make a payment: %r" % cur["lines"])
        ok("SK-52 %d frames fed through the serial port made the PSBT (progress showed parts/needed %s)" % (sent, total_seen))
        if scenario == "sign-refuse":
            other = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".split()
            for w in other:
                type_text(w)
                s = press("ret")
            if s["id"] != "sign-refused" or W["WORDS_NEEDED_SK_54_OTHER_WALLET"] not in s["lines"]:
                fail("a payment of another wallet was not refused: %r" % s["lines"])
            if any(l.startswith("bc1") or "bc1" in l for l in s["lines"]):
                fail("an address was shown before the refusal")
            ok("SK-53 words of another wallet: refused before anything is shown (%s)" % W["WORDS_NEEDED_SK_54_OTHER_WALLET"])
            shot("refused", 300)
        else:
            for w in TEST_SEED:
                type_text(w)
                s = press("ret")
            if s["id"] != "sign-review":
                fail("expected the review, got %s %r" % (s["id"], s["lines"]))
            info = {}
            for line in subprocess.run([pay_bin, "make", pay_dir], check=True, capture_output=True, text=True).stdout.splitlines():
                k, _, v = line.partition(" ")
                info[k] = v
            addr, amount = info["first-output"].split()
            text = "\n".join(s["lines"])
            unit = W["WORDS_NEEDED_SK_55_UNIT_BTC"]
            if ("1 " + addr) not in s["lines"] or (amount + " " + unit) not in s["lines"]:
                fail("the review does not show output 1 (%s %s): %r" % (addr, amount, s["lines"]))
            if not any(l.startswith(W["WORDS_NEEDED_SK_55_FEE"] + " " + info["fee"]) for l in s["lines"]):
                fail("the review's fee is not %s: %r" % (info["fee"], s["lines"]))
            if not any(l.startswith(W["WORDS_NEEDED_SK_55_SENT"] + " ") for l in s["lines"]):
                fail("no total sent away")
            if not any(l.startswith(W["WORDS_NEEDED_SK_55_FEE_RATE"] + " ") for l in s["lines"]):
                fail("no fee rate")
            own = sum(1 for l in s["lines"] if W["WORDS_NEEDED_SK_55_OWN"] in l)
            if own != 1:
                fail("expected one own output, got %d: %r" % (own, s["lines"]))
            ok("SK-55 review: output 1 %s %s, fee %s, one own output (the change), total and rate shown" % (addr, amount, info["fee"]))
            shot("review", 500)
            # Enter signs; the signed payment comes back as an animated code.
            n = len(screens())
            sendkey("ret")
            wait_screens(n)
            mats = []
            end = time.time() + 120
            need = 30
            while len(mats) < need and time.time() < end:
                allscr = screens()
                mats = [sc for sc in allscr if sc["id"] == "sign-out"]
                time.sleep(0.5)
            if len(mats) < need:
                fail("the signed payment's code did not animate (%d screens)" % len(mats))
            shot("signed", 3000)
            blocks, prev = [], None
            for sc in mats:
                if sc["qr"] != prev:
                    blocks.append("\n".join(sc["qr"]))
                    prev = sc["qr"]
            if len(blocks) < 2:
                fail("the code does not change between screens")
            ok("SK-56 the signed payment is shown as an animated code: %d screens, %d different" % (len(mats), len(blocks)))
            mpath = log_path + ".matrices"
            open(mpath, "w").write("\n\n".join(blocks) + "\n")
            out = subprocess.run([pay_bin, "verify", os.path.join(pay_dir, "sent.psbt"), form, mpath],
                                 capture_output=True, text=True)
            if out.returncode != 0 or "OK" not in out.stdout:
                fail("the signed payment does not check: %s %s" % (out.stdout, out.stderr))
            for line in out.stdout.splitlines():
                ok("SK-58 own-spend finishing: " + line)
            ok("SK-56 the signed payment came back in the form it went in (%s), signatures valid, file otherwise unchanged" % form)
else:
    fail("unknown scenario")

sendkey("esc")
ok("SK-29 Turn off pressed")
