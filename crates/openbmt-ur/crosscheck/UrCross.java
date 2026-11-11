// Cross-check of openbmt-ur against Hummingbird 1.7.6 (Sparrow's UR
// library), an independent implementation. Reads the cases printed by
// `cargo run --example crosscheck -- gen` and, for each case,
//
//   1. decodes the parts OpenBMT wrote with Hummingbird's URDecoder, in
//      order and then with 30% dropped, some repeated and the rest
//      shuffled (some in upper case): the type and the bytes must be the
//      same;
//   2. encodes the same message with Hummingbird's UREncoder and writes
//      its parts, in order and then with losses, for `crosscheck -- read`
//      to decode with OpenBMT's Decoder;
//   3. compares the first 2 * seqLen parts (the first one for a single
//      part) of both encoders character for character;
//   4. does the single-part cases both ways, in both cases of text, as
//      part of the above (a message that fits one part is one part).
//
//   java -cp <classes>:<hummingbird>:<cbor> UrCross cases.txt theirs.txt
//
// And, with `keys OUT`, the cosigner keys that Hummingbird's CryptoAccount and CryptoHDKey
// encode (shared wallet keys of three seeds, the shapes the signers write, a private key and
// the refusals), for `crosscheck keys OUT` to read with openbmt-ur.
//
// Exit status 1 on any difference; each is printed.

import com.sparrowwallet.hummingbird.UR;
import com.sparrowwallet.hummingbird.UREncoder;
import com.sparrowwallet.hummingbird.URDecoder;
import com.sparrowwallet.hummingbird.ResultType;
import com.sparrowwallet.hummingbird.registry.CryptoAccount;
import com.sparrowwallet.hummingbird.registry.CryptoCoinInfo;
import com.sparrowwallet.hummingbird.registry.CryptoHDKey;
import com.sparrowwallet.hummingbird.registry.CryptoKeypath;
import com.sparrowwallet.hummingbird.registry.CryptoOutput;
import com.sparrowwallet.hummingbird.registry.RegistryItem;
import com.sparrowwallet.hummingbird.registry.ScriptExpression;
import com.sparrowwallet.hummingbird.registry.pathcomponent.IndexPathComponent;
import com.sparrowwallet.hummingbird.registry.pathcomponent.PathComponent;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Random;

public class UrCross {
    static int failures = 0;
    static int cases = 0;
    static int partsCompared = 0;
    static int ordered = 0;
    static int lossy = 0;
    static int single = 0;

    static void fail(String what) {
        failures++;
        if (failures <= 40) {
            System.out.println("FAIL  " + what);
        }
    }

    static byte[] unhex(String s) {
        byte[] out = new byte[s.length() / 2];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) Integer.parseInt(s.substring(2 * i, 2 * i + 2), 16);
        }
        return out;
    }

    /** Feeds the parts to a fresh decoder; true when it gave the message. */
    static boolean decodes(List<String> parts, String type, byte[] payload, String label) {
        URDecoder d = new URDecoder();
        for (String p : parts) {
            try {
                d.receivePart(p);
            } catch (RuntimeException e) {
                // A part refused is not fatal for the lossy sequences.
            }
            URDecoder.Result r = d.getResult();
            if (r != null) {
                if (r.type != ResultType.SUCCESS) {
                    fail(label + ": Hummingbird failed: " + r.error);
                    return false;
                }
                if (!r.ur.getType().equals(type)
                        || !Arrays.equals(r.ur.getCborBytes(), payload)) {
                    fail(label + ": Hummingbird decoded another message");
                    return false;
                }
                return true;
            }
        }
        fail(label + ": Hummingbird did not complete in " + parts.size() + " parts");
        return false;
    }

    static List<String> lossy(List<String> parts, Random rnd) {
        List<String> kept = new ArrayList<>();
        for (String p : parts) {
            if (rnd.nextInt(100) >= 30) {
                kept.add(p);
            }
        }
        List<String> out = new ArrayList<>(kept);
        for (String p : kept) {
            if (rnd.nextInt(3) == 0) {
                out.add(p);
            }
        }
        Collections.shuffle(out, rnd);
        List<String> cased = new ArrayList<>();
        for (String p : out) {
            cased.add(rnd.nextBoolean() ? p.toUpperCase() : p);
        }
        return cased;
    }

    static void emit(PrintWriter w, String tag, List<String> parts) {
        for (String p : parts) {
            w.println(tag + " " + p);
        }
    }

    static void one(int idx, String type, int max, byte[] payload, List<String> ours,
            PrintWriter w) throws Exception {
        cases++;
        String label = "case " + idx + " (" + type + ", " + payload.length + " bytes, max " + max + ")";
        UREncoder enc = new UREncoder(new UR(type, payload), max, 10, 0);
        boolean singlePart = enc.isSinglePart();
        int seqLen = singlePart ? 1 : enc.getSeqLen();
        int total = singlePart ? 1 : Math.max(4 * seqLen, 32);
        List<String> theirs = new ArrayList<>();
        for (int i = 0; i < total; i++) {
            theirs.add(enc.nextPart());
        }
        if (singlePart) {
            single++;
        }
        if (ours.size() != total) {
            fail(label + ": " + ours.size() + " parts of ours, " + total + " of theirs");
            return;
        }
        // 3. The parts, character for character.
        int compare = singlePart ? 1 : 2 * seqLen;
        for (int i = 0; i < compare; i++) {
            partsCompared++;
            if (!ours.get(i).equals(theirs.get(i))) {
                fail(label + ": part " + (i + 1) + " differs\n   openbmt-ur  " + ours.get(i)
                        + "\n   Hummingbird " + theirs.get(i));
                break;
            }
        }
        // 1. Ours into Hummingbird.
        Random rnd = new Random(1000 + idx);
        List<String> inOrder = new ArrayList<>();
        for (int i = 0; i < ours.size(); i++) {
            inOrder.add(idx % 2 == 0 ? ours.get(i) : ours.get(i).toUpperCase());
        }
        if (decodes(inOrder, type, payload, label + " in order")) {
            ordered++;
        }
        if (singlePart) {
            // One part cannot be dropped.
            emit(w, "ordered", Arrays.asList(theirs.get(0)));
            emit(w, "lossy", Arrays.asList(theirs.get(0).toUpperCase()));
            return;
        }
        if (decodes(lossy(ours, rnd), type, payload, label + " lossy")) {
            lossy++;
        }
        // 2. Theirs into OpenBMT, written for the Rust side.
        List<String> head = new ArrayList<>();
        for (int i = 0; i < seqLen; i++) {
            head.add(idx % 2 == 0 ? theirs.get(i) : theirs.get(i).toUpperCase());
        }
        emit(w, "ordered", head);
        emit(w, "lossy", lossy(theirs, rnd));
    }

    // ---- cosigner keys (`keys OUT`) ----------------------------------------------------------

    /** One key of a fixed seed, with the text the key ceremony takes for it. */
    static final class KeySpec {
        final String name;
        final int coin;
        final String fp;
        final String parent;
        final String key;
        final String chain;
        /** The text from the independent BIP 32 tool that made the keys. */
        final String xpub;

        KeySpec(String name, int coin, String fp, String parent, String key, String chain,
                String xpub) {
            this.name = name;
            this.coin = coin;
            this.fp = fp;
            this.parent = parent;
            this.key = key;
            this.chain = chain;
            this.xpub = xpub;
        }
    }

    static final KeySpec[] KEYS = {
        new KeySpec("main", 0, "56c4fac3", "c07436a2",
                "02d44daf959d7b3bf9343e2aea84ea21391ccf306a2765c35859c94cecb870aaea",
                "f4ef1818131a780976e9dc1f30a4bd39903a9b8bbffad099035c49f886e85aed",
                "xpub6Ewx2N9hNSArJyF35CUGhaZLuZxQPNmJzWVwmpoV9U7Xu5wqka93nd3zEzokew9MzkNV4u6TCVDkHHR6QHQuYEFaasKzWkrkncXHMXGNdZP"),
        new KeySpec("test", 1, "56c4fac3", "686ae4bf",
                "0241ea5cb48ecc741308624baf221aa4e3057447056e4fa48749572714abff29f1",
                "d0b08dc903742e8e907aed44ced618ec55545adf33a8f3cfe444470bb19fe79b",
                "tpubDEg3kqr2jo5ergkJbFqRHvCpiob7wR7Hi44J7y987G1JZfbzBND77XKTyPZzGvh3uyDf8kexMJnFD9W8FuraJ4wLMsx6YuZVXRSRRcx6QdD"),
        new KeySpec("other", 0, "e2867bb6", "d51ad7f4",
                "037b73c092866bc4d0ed4a38bdeb8fd92e6f50da46b8e0df3ae383b2a84aeec26b",
                "364c446c3033c79f7b7b30617e8abaf7aa02b67cd15a294dcadcf0866c37d7a3",
                "xpub6F6kdziwgeYBty6BhQQxf73MxQDNPcKKApsvENDUEx3mBPumWC4GcBFi7gk8wE7r2V4gKRmC92UXPWPxz7HxP3U4n46xutvWWLsUAWswLWe"),
    };

    static final String ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    static int keyCases = 0;

    static String hex(byte[] b) {
        StringBuilder sb = new StringBuilder();
        for (byte x : b) {
            sb.append(String.format("%02x", x & 0xff));
        }
        return sb.toString();
    }

    static String base58Check(byte[] payload) throws Exception {
        java.security.MessageDigest sha = java.security.MessageDigest.getInstance("SHA-256");
        byte[] sum = sha.digest(sha.digest(payload));
        byte[] data = Arrays.copyOf(payload, payload.length + 4);
        System.arraycopy(sum, 0, data, payload.length, 4);
        java.math.BigInteger n = new java.math.BigInteger(1, data);
        java.math.BigInteger fiftyEight = java.math.BigInteger.valueOf(58);
        StringBuilder sb = new StringBuilder();
        while (n.signum() > 0) {
            java.math.BigInteger[] qr = n.divideAndRemainder(fiftyEight);
            sb.append(ALPHABET.charAt(qr[1].intValue()));
            n = qr[0];
        }
        for (byte b : data) {
            if (b != 0) {
                break;
            }
            sb.append('1');
        }
        return sb.reverse().toString();
    }

    /** The 78 bytes of BIP 32 for a key of the spec, written here and not by openbmt-ur. */
    static String serialize(KeySpec k) throws Exception {
        java.io.ByteArrayOutputStream o = new java.io.ByteArrayOutputStream();
        o.write(unhex(k.coin == 0 ? "0488b21e" : "043587cf"));
        o.write(4);
        o.write(unhex(k.parent));
        o.write(unhex("80000002"));
        o.write(unhex(k.chain));
        o.write(unhex(k.key));
        return base58Check(o.toByteArray());
    }

    static CryptoKeypath path(int coin, int account, String fp) {
        List<PathComponent> parts = new ArrayList<>();
        parts.add(new IndexPathComponent(48, true));
        parts.add(new IndexPathComponent(coin, true));
        parts.add(new IndexPathComponent(account, true));
        parts.add(new IndexPathComponent(2, true));
        return new CryptoKeypath(parts, unhex(fp), 4);
    }

    static CryptoHDKey hdkey(KeySpec k, Boolean isPrivate, boolean network, int account) {
        CryptoCoinInfo info = network
                ? new CryptoCoinInfo(CryptoCoinInfo.Type.BITCOIN,
                        k.coin == 0 ? CryptoCoinInfo.Network.MAINNET : CryptoCoinInfo.Network.TESTNET)
                : null;
        return new CryptoHDKey(isPrivate, unhex(k.key), unhex(k.chain), info,
                path(k.coin, account, k.fp), null, unhex(k.parent));
    }

    static void keyLine(PrintWriter w, String label, RegistryItem item, String want) {
        UR ur = item.toUR();
        w.println("key " + label + " " + ur.getType() + " " + hex(ur.getCborBytes()) + " " + want);
        keyCases++;
    }

    static List<CryptoOutput> one(List<ScriptExpression> script, CryptoHDKey key) {
        List<CryptoOutput> outputs = new ArrayList<>();
        outputs.add(new CryptoOutput(script, key));
        return outputs;
    }

    static final List<ScriptExpression> WSH = List.of(ScriptExpression.WITNESS_SCRIPT_HASH);
    static final List<ScriptExpression> WSH_COSIGNER =
            List.of(ScriptExpression.WITNESS_SCRIPT_HASH, ScriptExpression.COSIGNER);

    /**
     * Writes the keys that Hummingbird's CryptoAccount and CryptoHDKey encode, each with what a reader
     * must make of it: `OK <text>` or `REFUSE <rule>`. The text is made here from the key's parts.
     */
    static void keys(String out) throws Exception {
        try (PrintWriter w = new PrintWriter(Files.newBufferedWriter(Paths.get(out), StandardCharsets.UTF_8))) {
            for (KeySpec k : KEYS) {
                String text = serialize(k);
                if (!text.equals(k.xpub)) {
                    fail("key " + k.name + ": the text made here is " + text + ", not " + k.xpub);
                    continue;
                }
                String want = "OK [" + k.fp + "/48h/" + k.coin + "h/0h/2h]" + text;
                for (boolean network : new boolean[] {true, false}) {
                    if (!network && k.coin == 1) {
                        continue; // a key of a test network says so
                    }
                    for (Boolean priv : new Boolean[] {null, Boolean.FALSE}) {
                        CryptoHDKey hd = hdkey(k, priv, network, 0);
                        String label = k.name + (network ? "-net" : "-nonet") + "-" + priv;
                        // 303(hdkey) alone, as a crypto-hdkey.
                        keyLine(w, label + "-hdkey", hd, want);
                        // wsh(key) and wsh(cosigner(key)) in an account, as the signers write them.
                        for (List<ScriptExpression> script : List.of(WSH, WSH_COSIGNER)) {
                            keyLine(w, label + "-account-" + script.size(),
                                    new CryptoAccount(unhex(k.fp), one(script, hd)), want);
                            // The same, with the other outputs of an account around it.
                            CryptoHDKey single = new CryptoHDKey(priv, unhex(k.key), unhex(k.chain),
                                    null, new CryptoKeypath(List.of(new IndexPathComponent(84, true),
                                            new IndexPathComponent(k.coin, true),
                                            new IndexPathComponent(0, true)), unhex(k.fp), 3),
                                    null, unhex(k.parent));
                            List<CryptoOutput> mixed = new ArrayList<>();
                            mixed.add(new CryptoOutput(List.of(ScriptExpression.WITNESS_PUBLIC_KEY_HASH), single));
                            mixed.add(new CryptoOutput(List.of(ScriptExpression.SCRIPT_HASH,
                                    ScriptExpression.WITNESS_PUBLIC_KEY_HASH), single));
                            mixed.add(new CryptoOutput(script, hd));
                            mixed.add(new CryptoOutput(List.of(ScriptExpression.SCRIPT_HASH,
                                    ScriptExpression.WITNESS_SCRIPT_HASH), hd));
                            keyLine(w, label + "-mixed-" + script.size(),
                                    new CryptoAccount(unhex(k.fp), mixed), want);
                        }
                    }
                }
                // A key that says it is private is never taken, in any wrapping.
                CryptoHDKey secret = hdkey(k, Boolean.TRUE, true, 0);
                keyLine(w, k.name + "-private-hdkey", secret, "REFUSE UR-26");
                keyLine(w, k.name + "-private-account",
                        new CryptoAccount(unhex(k.fp), one(WSH, secret)), "REFUSE UR-26");
                // The second account of the wallet has no key for the shared wallet.
                keyLine(w, k.name + "-account-1",
                        new CryptoAccount(unhex(k.fp), one(WSH, hdkey(k, Boolean.FALSE, true, 1))),
                        "REFUSE UR-25");
                // Two keys for the shared wallet in one account are not told apart.
                List<CryptoOutput> two = one(WSH, hdkey(k, Boolean.FALSE, true, 0));
                two.add(new CryptoOutput(WSH_COSIGNER, hdkey(k, null, false, 0)));
                keyLine(w, k.name + "-two-keys", new CryptoAccount(unhex(k.fp), two), "REFUSE UR-25");
                // The fingerprint of the account must be the one of the origin.
                keyLine(w, k.name + "-fingerprints",
                        new CryptoAccount(unhex("01020304"), one(WSH, hdkey(k, null, true, 0))),
                        "REFUSE UR-27");
            }
        }
        System.out.println("keys written for openbmt-ur: " + keyCases);
        System.out.println(failures == 0 ? "PASS  Hummingbird side (keys)" : "FAILURES: " + failures);
        if (failures != 0) {
            System.exit(1);
        }
    }

    public static void main(String[] args) throws Exception {
        if (args.length == 2 && args[0].equals("keys")) {
            keys(args[1]);
            return;
        }
        List<String> ours = new ArrayList<>();
        String type = null;
        int idx = 0;
        int max = 0;
        byte[] payload = null;
        try (BufferedReader r = Files.newBufferedReader(Paths.get(args[0]), StandardCharsets.UTF_8);
                PrintWriter w = new PrintWriter(Files.newBufferedWriter(Paths.get(args[1]), StandardCharsets.UTF_8))) {
            String line;
            while ((line = r.readLine()) != null) {
                String[] f = line.split(" ");
                if (f[0].equals("case")) {
                    idx = Integer.parseInt(f[1]);
                    type = f[2];
                    max = Integer.parseInt(f[3]);
                    payload = unhex(f[4]);
                    ours = new ArrayList<>();
                    w.println("case " + idx);
                } else if (f[0].equals("ours")) {
                    ours.add(f[1]);
                } else if (f[0].equals("end")) {
                    try {
                        one(idx, type, max, payload, ours, w);
                    } catch (Exception e) {
                        fail("case " + idx + ": " + e);
                    }
                    w.println("end");
                }
            }
        }
        System.out.println("cases: " + cases + " (" + single + " single-part)");
        System.out.println("parts compared character for character: " + partsCompared);
        System.out.println("openbmt-ur parts decoded by Hummingbird: in order " + ordered
                + ", with 30% lost " + lossy);
        System.out.println(failures == 0 ? "PASS  Hummingbird side" : "FAILURES: " + failures);
        if (failures != 0) {
            System.exit(1);
        }
    }
}
