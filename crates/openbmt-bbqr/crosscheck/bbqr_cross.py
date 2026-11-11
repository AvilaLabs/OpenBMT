"""Cross-check harness of crates/openbmt-bbqr against Coinkite's reference
implementation (python/bbqr of https://github.com/coinkite/BBQr, public
domain), which scripts/bbqr-crosscheck.sh fetches at a pinned commit and
checks against pinned SHA-256 values. A test tool: never shipped.

  bbqr_cross.py make REFDIR FIXTURES OUT
      Writes the cases: the fixture files of the reference repository and a
      few made-up inputs, each with the parts the REFERENCE writes in the
      encodings H, 2 and Z at several part sizes.
  bbqr_cross.py verify REFDIR CASES OURS
      Reads the parts openbmt-bbqr wrote for the same cases (OURS) with the
      REFERENCE's reader and compares them with the data.

The line formats are the ones of examples/crosscheck.rs:

  case <name> <kind> <hex of the data>
  parts <label>
  <one part per line>
"""

import importlib
import os
import sys


def load_reference(refdir):
    # The reference's modules use relative imports, so they must stand in a
    # package. The package marker is made here, empty.
    pkg = os.path.join(refdir, "bbqr")
    os.makedirs(pkg, exist_ok=True)
    marker = os.path.join(pkg, "__init__.py")
    if not os.path.exists(marker):
        open(marker, "w").close()
    sys.path.insert(0, refdir)
    utils = importlib.import_module("bbqr.utils")
    join = importlib.import_module("bbqr.join")
    return utils, join


def synthetic():
    out = []
    state = 12345

    def rnd(n):
        nonlocal state
        b = bytearray()
        for _ in range(n):
            state = (state * 1103515245 + 12345) & 0x7FFFFFFF
            b.append((state >> 16) & 0xFF)
        return bytes(b)

    for n in (1, 2, 3, 4, 5, 6, 7, 8, 9, 31, 32, 33, 40, 41, 100, 999, 5000):
        out.append(("random-%d" % n, "P", rnd(n)))
    text = (b"witness script sorted multi input output bip32 xpub " * 120)
    out.append(("text-6k", "T", text))
    out.append(("zeros-3k", "P", bytes(3000)))
    return out


def reference_parts(utils, raw, kind, encoding, chars):
    enc, data, split_mod = utils.encode_data(raw, encoding)
    room = chars - 8
    cap2 = room - (room % split_mod)
    n = -(-len(data) // cap2)
    if n > 1295:
        return None
    parts = []
    for i in range(n):
        chunk = data[i * cap2:(i + 1) * cap2]
        parts.append("B$%s%s%s%s%s" % (enc, kind, utils.int2base36(n), utils.int2base36(i), chunk))
    return parts


def make(refdir, fixtures, out):
    utils, _ = load_reference(refdir)
    cases = synthetic()
    for name in sorted(os.listdir(fixtures)):
        path = os.path.join(fixtures, name)
        if name.endswith(".psbt"):
            cases.append((name, "P", open(path, "rb").read()))
        elif name.endswith(".txn"):
            cases.append((name, "T", open(path, "rb").read()))
    lines = []
    count = 0
    for name, kind, raw in cases:
        lines.append("case %s %s %s" % (name, kind, raw.hex()))
        for encoding in ("H", "2", "Z"):
            for chars in (100, 400, 1000, 2132, 4296):
                parts = reference_parts(utils, raw, kind, encoding, chars)
                if parts is None:
                    continue
                lines.append("parts reference-%s-%d" % (encoding, chars))
                lines.extend(parts)
                count += 1
    with open(out, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("cases: %d, sets of parts: %d" % (len(cases), count))


def read_cases(path):
    cases = []
    cur = None
    for line in open(path):
        line = line.rstrip("\n")
        if line.startswith("case "):
            _, name, kind, hx = line.split(" ")
            cur = {"name": name, "kind": kind, "data": bytes.fromhex(hx), "sets": []}
            cases.append(cur)
        elif line.startswith("parts "):
            cur["sets"].append((line[6:], []))
        elif line:
            cur["sets"][-1][1].append(line)
    return cases


def verify(refdir, cases_path, ours_path):
    _, join = load_reference(refdir)
    want = {c["name"]: c for c in read_cases(cases_path)}
    checked = 0
    for c in read_cases(ours_path):
        expect = want[c["name"]]
        assert c["data"] == expect["data"], "the data of %s changed" % c["name"]
        for label, parts in c["sets"]:
            kind, raw = join.join_qrs(parts)
            assert kind == c["kind"], "%s %s: type %s" % (c["name"], label, kind)
            assert raw == c["data"], "%s %s: the reference read other bytes" % (c["name"], label)
            # Any order too.
            kind, raw = join.join_qrs(list(reversed(parts)))
            assert raw == c["data"], "%s %s: reversed" % (c["name"], label)
            checked += 1
    print("PASS  the reference read %d sets written by openbmt-bbqr" % checked)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "make":
        make(sys.argv[2], sys.argv[3], sys.argv[4])
    elif cmd == "verify":
        verify(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit("unknown command " + cmd)
