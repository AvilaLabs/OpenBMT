package com.google.zxing.qrcode.encoder;

// ZXing's MaskUtil is package-private. This class sits in its package, in
// the harness only, to read the penalty of a matrix as the sum of its four
// rules (the sum ZXing's own mask choice compares).
public final class PenaltyAccess {
    private PenaltyAccess() {}

    public static int penalty(ByteMatrix matrix) {
        return MaskUtil.applyMaskPenaltyRule1(matrix)
                + MaskUtil.applyMaskPenaltyRule2(matrix)
                + MaskUtil.applyMaskPenaltyRule3(matrix)
                + MaskUtil.applyMaskPenaltyRule4(matrix);
    }
}
