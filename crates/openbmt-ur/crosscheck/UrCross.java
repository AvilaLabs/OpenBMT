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
// Exit status 1 on any difference; each is printed.

import com.sparrowwallet.hummingbird.UR;
import com.sparrowwallet.hummingbird.UREncoder;
import com.sparrowwallet.hummingbird.URDecoder;
import com.sparrowwallet.hummingbird.ResultType;

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
                fail(label + ": part " + (i + 1) + " differs\n   ours   " + ours.get(i)
                        + "\n   theirs " + theirs.get(i));
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

    public static void main(String[] args) throws Exception {
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
        System.out.println("ours decoded by Hummingbird: in order " + ordered
                + ", with 30% lost " + lossy);
        System.out.println(failures == 0 ? "PASS  Hummingbird side" : "FAILURES: " + failures);
        if (failures != 0) {
            System.exit(1);
        }
    }
}
