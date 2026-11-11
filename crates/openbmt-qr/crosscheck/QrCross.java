// Cross-check of openbmt-qr against ZXing (core 3.5.3), an independent
// implementation. Reads the cases printed by `cargo run --example
// crosscheck` on standard input. For each case it
//
//   1. encodes the same input with ZXing's Encoder, forcing the same
//      version, level and mask, and compares the matrix module by module;
//   2. decodes our matrix with ZXing's QRCodeReader (4-module quiet zone,
//      PURE_BARCODE) and compares the text, or for byte mode the raw byte
//      segments, with the input, and the level with ours;
//   3. for the cases where the encoder chose the version and the mask:
//      encodes with ZXing with only the level forced and compares the
//      version and the mask it picks, then compares the penalty of every
//      mask (ZXing's MaskUtil, through PenaltyAccess, against ours).
//
// Steps 1 and 2 must hold for every case: any miss is a failure (exit 1).
// Step 3 is reported only: where the two choose a different mask the
// penalties say which rule they read differently.
//
// Byte mode: the input is turned into a String by ISO-8859-1, which maps
// every byte to one char, and no CHARACTER_SET hint is given, because the
// hint makes ZXing add an ECI segment. Without it ZXing writes the
// ISO-8859-1 bytes as they are, with no ECI, like this encoder.

import com.google.zxing.BinaryBitmap;
import com.google.zxing.DecodeHintType;
import com.google.zxing.EncodeHintType;
import com.google.zxing.RGBLuminanceSource;
import com.google.zxing.Result;
import com.google.zxing.ResultMetadataType;
import com.google.zxing.common.HybridBinarizer;
import com.google.zxing.qrcode.QRCodeReader;
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel;
import com.google.zxing.qrcode.encoder.ByteMatrix;
import com.google.zxing.qrcode.encoder.Encoder;
import com.google.zxing.qrcode.encoder.PenaltyAccess;
import com.google.zxing.qrcode.encoder.QRCode;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.EnumMap;
import java.util.List;
import java.util.Map;

public class QrCross {
    static final int QUIET = 4;
    static final int SCALE = 2;

    static int cases, forced, auto;
    static int matrixOk, matrixBad, decodeOk, decodeBad, errors;
    static int autoVersionSame, autoVersionDiff, autoMaskSame, autoMaskDiff;
    static int penaltiesSame, penaltiesDiff;
    static final int[] byMode = new int[3];
    static final List<String> failures = new ArrayList<>();
    static final List<String> differences = new ArrayList<>();

    static ErrorCorrectionLevel level(String s) {
        switch (s) {
            case "L": return ErrorCorrectionLevel.L;
            case "M": return ErrorCorrectionLevel.M;
            case "Q": return ErrorCorrectionLevel.Q;
            default: return ErrorCorrectionLevel.H;
        }
    }

    static byte[] unhex(String h) {
        byte[] out = new byte[h.length() / 2];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) Integer.parseInt(h.substring(2 * i, 2 * i + 2), 16);
        }
        return out;
    }

    static String field(String[] parts, String key) {
        for (String p : parts) {
            if (p.startsWith(key + "=")) return p.substring(key.length() + 1);
        }
        throw new IllegalArgumentException("no " + key);
    }

    static QRCode zx(String content, ErrorCorrectionLevel ec, Integer version, Integer mask)
            throws Exception {
        Map<EncodeHintType, Object> hints = new EnumMap<>(EncodeHintType.class);
        hints.put(EncodeHintType.ERROR_CORRECTION, ec);
        if (version != null) hints.put(EncodeHintType.QR_VERSION, version);
        if (mask != null) hints.put(EncodeHintType.QR_MASK_PATTERN, mask);
        return Encoder.encode(content, ec, hints);
    }

    static void fail(String id, String why) {
        if (failures.size() < 20) failures.add("case " + id + ": " + why);
    }

    public static void main(String[] args) throws Exception {
        BufferedReader in =
                new BufferedReader(new InputStreamReader(System.in, StandardCharsets.ISO_8859_1));
        String line;
        while ((line = in.readLine()) != null) {
            if (line.isEmpty()) continue;
            String[] head = line.split(" ");
            String id = head[1];
            String kind = field(head, "kind");
            String mode = field(head, "mode");
            ErrorCorrectionLevel ec = level(field(head, "ecc"));
            int version = Integer.parseInt(field(head, "version"));
            int mask = Integer.parseInt(field(head, "mask"));
            byte[] data = unhex(in.readLine().substring(5));
            String pen = in.readLine().substring(10);
            int size = 17 + 4 * version;
            boolean[][] ours = new boolean[size][size];
            for (int y = 0; y < size; y++) {
                String row = in.readLine();
                if (row.length() != size) throw new IllegalStateException("short row in case " + id);
                for (int x = 0; x < size; x++) ours[y][x] = row.charAt(x) == '1';
            }
            cases++;
            if (kind.equals("forced")) forced++; else auto++;
            byMode[mode.equals("numeric") ? 0 : mode.equals("alphanumeric") ? 1 : 2]++;
            String content = new String(data, StandardCharsets.ISO_8859_1);

            // 1. Like for like: same version, level and mask.
            try {
                QRCode q = zx(content, ec, version, mask);
                ByteMatrix m = q.getMatrix();
                int bad = 0;
                if (m.getWidth() != size) bad = -1;
                else {
                    for (int y = 0; y < size; y++)
                        for (int x = 0; x < size; x++)
                            if ((m.get(x, y) == 1) != ours[y][x]) bad++;
                }
                if (bad == 0 && q.getMaskPattern() == mask && q.getVersion().getVersionNumber() == version) {
                    matrixOk++;
                } else {
                    matrixBad++;
                    fail(id, "matrix differs in " + bad + " modules (" + mode + " " + head[4] + " v" + version + " mask " + mask + ")");
                }
            } catch (Exception e) {
                errors++;
                fail(id, "ZXing could not encode: " + e);
            }

            // 2. Decode our matrix.
            try {
                int w = (size + 2 * QUIET) * SCALE;
                int[] px = new int[w * w];
                java.util.Arrays.fill(px, 0xFFFFFF);
                for (int y = 0; y < size; y++)
                    for (int x = 0; x < size; x++)
                        if (ours[y][x])
                            for (int dy = 0; dy < SCALE; dy++)
                                for (int dx = 0; dx < SCALE; dx++)
                                    px[((y + QUIET) * SCALE + dy) * w + (x + QUIET) * SCALE + dx] = 0x000000;
                Map<DecodeHintType, Object> dh = new EnumMap<>(DecodeHintType.class);
                dh.put(DecodeHintType.PURE_BARCODE, Boolean.TRUE);
                Result r = new QRCodeReader().decode(
                        new BinaryBitmap(new HybridBinarizer(new RGBLuminanceSource(w, w, px))), dh);
                boolean same;
                if (mode.equals("byte")) {
                    @SuppressWarnings("unchecked")
                    List<byte[]> segs = (List<byte[]>) r.getResultMetadata().get(ResultMetadataType.BYTE_SEGMENTS);
                    java.io.ByteArrayOutputStream all = new java.io.ByteArrayOutputStream();
                    if (segs != null) for (byte[] s : segs) all.write(s);
                    same = java.util.Arrays.equals(all.toByteArray(), data);
                } else {
                    same = r.getText().equals(content);
                }
                String level = String.valueOf(r.getResultMetadata().get(ResultMetadataType.ERROR_CORRECTION_LEVEL));
                if (same && level.equals(field(head, "ecc"))) decodeOk++;
                else {
                    decodeBad++;
                    fail(id, "decode differs (same data " + same + ", level " + level + ")");
                }
            } catch (Exception e) {
                decodeBad++;
                fail(id, "ZXing could not decode: " + e);
            }

            // 3. Free choice of version and mask.
            if (kind.equals("auto")) {
                try {
                    QRCode free = zx(content, ec, null, null);
                    int zv = free.getVersion().getVersionNumber();
                    if (zv == version) autoVersionSame++;
                    else {
                        autoVersionDiff++;
                        differences.add("case " + id + ": version ours " + version + ", ZXing " + zv);
                    }
                    // The mask ZXing picks at our version, and the penalty
                    // of every mask on each side.
                    QRCode atOurs = zx(content, ec, version, null);
                    int zm = atOurs.getMaskPattern();
                    if (zm == mask) autoMaskSame++;
                    else {
                        autoMaskDiff++;
                        differences.add("case " + id + ": mask ours " + mask + ", ZXing " + zm
                                + " (" + mode + " " + head[4] + " v" + version + ")");
                    }
                    String[] op = pen.split(" ");
                    boolean allSame = true;
                    StringBuilder zp = new StringBuilder();
                    for (int m = 0; m < 8; m++) {
                        int z = PenaltyAccess.penalty(zx(content, ec, version, m).getMatrix());
                        zp.append(z).append(' ');
                        if (z != Integer.parseInt(op[m])) allSame = false;
                    }
                    if (allSame) penaltiesSame++;
                    else {
                        penaltiesDiff++;
                        differences.add("case " + id + ": penalties ours [" + pen + "] ZXing [" + zp.toString().trim() + "]");
                    }
                } catch (Exception e) {
                    errors++;
                    fail(id, "ZXing free encode failed: " + e);
                }
            }
        }
        System.out.println("cases: " + cases + " (" + forced + " forced, " + auto + " auto; numeric "
                + byMode[0] + ", alphanumeric " + byMode[1] + ", byte " + byMode[2] + ")");
        System.out.println("1 matrix equal to ZXing's, same version/level/mask: " + matrixOk + " ok, " + matrixBad + " different");
        System.out.println("2 decoded by ZXing to the input: " + decodeOk + " ok, " + decodeBad + " different");
        System.out.println("3 version chosen by ZXing equals ours: " + autoVersionSame + " same, " + autoVersionDiff + " different");
        System.out.println("3 mask chosen by ZXing equals ours: " + autoMaskSame + " same, " + autoMaskDiff + " different");
        System.out.println("3 all eight mask penalties equal ZXing's: " + penaltiesSame + " cases, " + penaltiesDiff + " cases differ");
        System.out.println("ZXing errors: " + errors);
        for (String d : differences.subList(0, Math.min(differences.size(), 30))) System.out.println("note " + d);
        for (String f : failures) System.out.println("FAIL " + f);
        boolean bad = matrixBad > 0 || decodeBad > 0 || errors > 0 || cases == 0;
        System.out.println(bad ? "CROSS-CHECK FAILED" : "CROSS-CHECK PASSED");
        System.exit(bad ? 1 : 0);
    }
}
