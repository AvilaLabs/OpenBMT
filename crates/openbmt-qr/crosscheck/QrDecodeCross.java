// The reader's cross-check against ZXing (core 3.5.3). Two commands.
//
//   gen DIR N    makes N images of codes encoded by ZXing: random content
//                (digits, upper-case text, Latin-1 bytes, or UTF-8 text
//                with an ECI), a random level, module size 2 to 6, a quiet
//                zone of 4, turned by a multiple of 90 degrees. Each is
//                zx_I.pgm with zx_I.txt holding the expected data in hex.
//   check DIR    after `decodecheck read DIR` has written NAME.out for
//                every image:
//                  1. our reader on ZXing's images: the data must be the
//                     content ("zx" images);
//                  2. ZXing on our images ("ours_clean"): the data must be
//                     what we encoded, and ours the same as ZXing's;
//                  3. the same for our lower-quality images ("ours_hard"),
//                     reported only: neither reader is required to read
//                     them, but where both do, they must agree.
//                Steps 1 and 2 must be complete: any miss is a failure.
//
// Data is compared as bytes. ZXing's raw byte segments are used where it
// gives them (byte mode, with or without an ECI), else its text.

import com.google.zxing.BinaryBitmap;
import com.google.zxing.DecodeHintType;
import com.google.zxing.EncodeHintType;
import com.google.zxing.LuminanceSource;
import com.google.zxing.RGBLuminanceSource;
import com.google.zxing.Result;
import com.google.zxing.ResultMetadataType;
import com.google.zxing.common.HybridBinarizer;
import com.google.zxing.qrcode.QRCodeReader;
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel;
import com.google.zxing.qrcode.encoder.ByteMatrix;
import com.google.zxing.qrcode.encoder.Encoder;
import com.google.zxing.qrcode.encoder.QRCode;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.EnumMap;
import java.util.List;
import java.util.Map;
import java.util.Random;

public class QrDecodeCross {
    static final String ALNUM = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

    static String hex(byte[] b) {
        StringBuilder s = new StringBuilder();
        for (byte x : b) s.append(String.format("%02x", x & 0xFF));
        return s.toString();
    }

    static byte[] unhex(String h) {
        byte[] out = new byte[h.length() / 2];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) Integer.parseInt(h.substring(2 * i, 2 * i + 2), 16);
        }
        return out;
    }

    static void writePgm(File f, int w, int h, int[] gray) throws Exception {
        byte[] head = ("P5\n" + w + " " + h + "\n255\n").getBytes(StandardCharsets.US_ASCII);
        byte[] out = new byte[head.length + w * h];
        System.arraycopy(head, 0, out, 0, head.length);
        for (int i = 0; i < w * h; i++) out[head.length + i] = (byte) gray[i];
        Files.write(f.toPath(), out);
    }

    // Returns {w, h, pixels as 0..255}.
    static int[][] readPgm(File f) throws Exception {
        byte[] b = Files.readAllBytes(f.toPath());
        int pos = 0;
        int[] v = new int[4];
        for (int t = 0; t < 4; t++) {
            while (Character.isWhitespace(b[pos])) pos++;
            int start = pos;
            while (!Character.isWhitespace(b[pos])) pos++;
            String tok = new String(b, start, pos - start, StandardCharsets.US_ASCII);
            v[t] = t == 0 ? 0 : Integer.parseInt(tok);
        }
        pos++;
        int w = v[1], h = v[2];
        int[] px = new int[w * h];
        for (int i = 0; i < w * h; i++) px[i] = b[pos + i] & 0xFF;
        return new int[][] {{w, h}, px};
    }

    static ErrorCorrectionLevel level(int i) {
        return new ErrorCorrectionLevel[] {
            ErrorCorrectionLevel.L, ErrorCorrectionLevel.M, ErrorCorrectionLevel.Q, ErrorCorrectionLevel.H
        }[i];
    }

    static void gen(String dir, int n) throws Exception {
        File d = new File(dir);
        d.mkdirs();
        Random rng = new Random(0x0BD70002L);
        for (int i = 0; i < n; i++) {
            int kind = i % 4;
            byte[] expected;
            String content;
            Map<EncodeHintType, Object> hints = new EnumMap<>(EncodeHintType.class);
            ErrorCorrectionLevel ec = level(rng.nextInt(4));
            hints.put(EncodeHintType.ERROR_CORRECTION, ec);
            if (kind == 0) {
                int len = 1 + rng.nextInt(400);
                StringBuilder s = new StringBuilder();
                for (int k = 0; k < len; k++) s.append((char) ('0' + rng.nextInt(10)));
                content = s.toString();
                expected = content.getBytes(StandardCharsets.ISO_8859_1);
            } else if (kind == 1) {
                int len = 1 + rng.nextInt(250);
                StringBuilder s = new StringBuilder();
                for (int k = 0; k < len; k++) s.append(ALNUM.charAt(rng.nextInt(45)));
                content = s.toString();
                expected = content.getBytes(StandardCharsets.ISO_8859_1);
            } else if (kind == 2) {
                int len = 1 + rng.nextInt(150);
                byte[] raw = new byte[len];
                for (int k = 0; k < len; k++) raw[k] = (byte) (0x80 + rng.nextInt(0x80));
                content = new String(raw, StandardCharsets.ISO_8859_1);
                expected = raw;
            } else {
                int len = 1 + rng.nextInt(60);
                StringBuilder s = new StringBuilder();
                for (int k = 0; k < len; k++) {
                    int pick = rng.nextInt(4);
                    int cp = pick == 0 ? 0x61 + rng.nextInt(26)
                            : pick == 1 ? 0xC0 + rng.nextInt(0x3F)
                            : pick == 2 ? 0x391 + rng.nextInt(0x30)
                            : 0x4E00 + rng.nextInt(0x200);
                    s.appendCodePoint(cp);
                }
                content = s.toString();
                expected = content.getBytes(StandardCharsets.UTF_8);
                hints.put(EncodeHintType.CHARACTER_SET, "UTF-8");
            }
            QRCode q = Encoder.encode(content, ec, hints);
            ByteMatrix m = q.getMatrix();
            int size = m.getWidth();
            int scale = 2 + rng.nextInt(5);
            int w = (size + 8) * scale;
            int[] px = new int[w * w];
            Arrays.fill(px, 255);
            for (int y = 0; y < size; y++)
                for (int x = 0; x < size; x++)
                    if (m.get(x, y) == 1)
                        for (int dy = 0; dy < scale; dy++)
                            for (int dx = 0; dx < scale; dx++)
                                px[((y + 4) * scale + dy) * w + (x + 4) * scale + dx] = 0;
            for (int turns = rng.nextInt(4); turns > 0; turns--) {
                int[] r = new int[w * w];
                for (int y = 0; y < w; y++)
                    for (int x = 0; x < w; x++) r[x * w + (w - 1 - y)] = px[y * w + x];
                px = r;
            }
            writePgm(new File(d, "zx_" + i + ".pgm"), w, w, px);
            Files.write(new File(d, "zx_" + i + ".txt").toPath(), hex(expected).getBytes(StandardCharsets.US_ASCII));
        }
    }

    static byte[] zxingRead(int w, int h, int[] gray) throws Exception {
        int[] rgb = new int[w * h];
        for (int i = 0; i < w * h; i++) rgb[i] = (gray[i] << 16) | (gray[i] << 8) | gray[i];
        LuminanceSource src = new RGBLuminanceSource(w, h, rgb);
        Map<DecodeHintType, Object> hints = new EnumMap<>(DecodeHintType.class);
        hints.put(DecodeHintType.TRY_HARDER, Boolean.TRUE);
        Result r = new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(src)), hints);
        @SuppressWarnings("unchecked")
        List<byte[]> segs = (List<byte[]>) r.getResultMetadata().get(ResultMetadataType.BYTE_SEGMENTS);
        if (segs != null && !segs.isEmpty()) {
            ByteArrayOutputStream all = new ByteArrayOutputStream();
            for (byte[] s : segs) all.write(s);
            return all.toByteArray();
        }
        return r.getText().getBytes(StandardCharsets.ISO_8859_1);
    }

    static String slurp(File f) throws Exception {
        return new String(Files.readAllBytes(f.toPath()), StandardCharsets.US_ASCII).trim();
    }

    public static void main(String[] args) throws Exception {
        if (args[0].equals("gen")) {
            gen(args[1], Integer.parseInt(args[2]));
            return;
        }
        File dir = new File(args[1]);
        String[] names = dir.list((d, n) -> n.endsWith(".pgm"));
        Arrays.sort(names);
        int zxTotal = 0, zxOurOk = 0;
        int cleanTotal = 0, cleanZxOk = 0, cleanOurOk = 0, cleanAgree = 0;
        int hardTotal = 0, hardZx = 0, hardOurs = 0, hardBoth = 0, hardAgree = 0, hardDiffer = 0;
        List<String> failures = new ArrayList<>();
        for (String name : names) {
            String base = name.substring(0, name.length() - 4);
            String expected = slurp(new File(dir, base + ".txt"));
            String ours = slurp(new File(dir, base + ".out"));
            boolean oursOk = ours.equals("OK " + expected);
            if (base.startsWith("zx_")) {
                zxTotal++;
                if (oursOk) zxOurOk++;
                else if (failures.size() < 20) failures.add(base + ": ours said " + ours.substring(0, Math.min(60, ours.length())));
                continue;
            }
            int[][] img = readPgm(new File(dir, name));
            byte[] zx = null;
            try {
                zx = zxingRead(img[0][0], img[0][1], img[1]);
            } catch (Exception e) {
                zx = null;
            }
            boolean zxOk = zx != null && hex(zx).equals(expected);
            boolean oursAny = ours.startsWith("OK ");
            if (base.startsWith("ours_clean_")) {
                cleanTotal++;
                if (zxOk) cleanZxOk++;
                else if (failures.size() < 20) failures.add(base + ": ZXing " + (zx == null ? "no read" : "different data"));
                if (oursOk) cleanOurOk++;
                else if (failures.size() < 20) failures.add(base + ": ours said " + ours.substring(0, Math.min(60, ours.length())));
                if (zx != null && oursAny && ours.equals("OK " + hex(zx))) cleanAgree++;
            } else {
                hardTotal++;
                if (zxOk) hardZx++;
                if (oursOk) hardOurs++;
                if (zxOk && oursOk) hardBoth++;
                if (zx != null && oursAny) {
                    if (ours.equals("OK " + hex(zx))) hardAgree++;
                    else {
                        hardDiffer++;
                        if (failures.size() < 20) failures.add(base + ": both read and they differ");
                    }
                }
            }
        }
        System.out.println("1 ZXing's images read by ours: " + zxOurOk + "/" + zxTotal);
        System.out.println("2 our images read by ZXing: " + cleanZxOk + "/" + cleanTotal
                + ", by ours: " + cleanOurOk + "/" + cleanTotal + ", same data from both: " + cleanAgree + "/" + cleanTotal);
        System.out.println("3 our lower-quality images: ZXing reads " + hardZx + "/" + hardTotal
                + ", ours " + hardOurs + "/" + hardTotal + ", both " + hardBoth
                + ", where both read, same data: " + hardAgree);
        for (String f : failures) System.out.println("FAIL " + f);
        // Where both read a lower-quality image they must agree.
        boolean bad = zxTotal == 0 || zxOurOk != zxTotal || cleanZxOk != cleanTotal
                || cleanOurOk != cleanTotal || cleanAgree != cleanTotal || hardDiffer > 0;
        System.out.println(bad ? "DECODE CROSS-CHECK FAILED" : "DECODE CROSS-CHECK PASSED");
        System.exit(bad ? 1 : 0);
    }
}
