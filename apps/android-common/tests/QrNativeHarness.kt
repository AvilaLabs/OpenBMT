package org.openbmt.app

import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.openbmt.common.QrNative

/**
 * Loads the host-built `libopenbmt_qr_jni.so` and reads frames made by the repository's encoder
 * (`crates/openbmt-qr-jni/examples/make_frame.rs`) through the same Kotlin calls the camera scan makes:
 * the plane is copied with [QrFrames.copyLuma], handed to [QrNative.decode], and the answer is parsed
 * with [QrReply.parse]. Arguments: the library path, then pairs of a frame file and the text it holds.
 */
object QrNativeHarness {
    private var count = 0

    private fun ok(cond: Boolean, what: String) {
        count++
        if (!cond) throw AssertionError("FAILED: $what")
    }

    private fun read(frame: File): Triple<Int, Int, Pair<Int, ByteArray>> {
        val all = ByteBuffer.wrap(frame.readBytes()).order(ByteOrder.LITTLE_ENDIAN)
        val w = all.int
        val h = all.int
        val stride = all.int
        val plane = ByteArray(all.remaining())
        all.get(plane)
        return Triple(w, h, stride to plane)
    }

    @JvmStatic
    fun main(args: Array<String>) {
        System.load(File(args[0]).absolutePath)
        for (i in 1 until args.size step 2) {
            val (w, h, sp) = read(File(args[i]))
            val (stride, plane) = sp
            val expected = args[i + 1]
            val name = File(args[i]).name

            val strided = QrFrames.copyLuma(ByteBuffer.wrap(plane), w, h, stride, 1, false)!!
            val a = QrReply.parse(QrNative.decode(w, h, strided.stride, strided.bytes))
            ok(a is QrReply.Decoded && String(a.bytes, Charsets.UTF_8) == expected, "$name read with its stride")

            val tight = QrFrames.copyLuma(ByteBuffer.wrap(plane), w, h, stride, 1, true)!!
            ok(tight.stride == w, "$name tight rows")
            val b = QrReply.parse(QrNative.decode(w, h, tight.stride, tight.bytes))
            ok(b is QrReply.Decoded && String(b.bytes, Charsets.UTF_8) == expected, "$name read tight")

            // The same plane with every other byte between its pixels (pixel stride 2), gathered.
            val spread = ByteArray(stride * 2 * h)
            for (y in 0 until h) for (x in 0 until w) spread[y * stride * 2 + x * 2] = plane[y * stride + x]
            val gathered = QrFrames.copyLuma(ByteBuffer.wrap(spread), w, h, stride * 2, 2, false)!!
            val c = QrReply.parse(QrNative.decode(w, h, gathered.stride, gathered.bytes))
            ok(c is QrReply.Decoded && String(c.bytes, Charsets.UTF_8) == expected, "$name read after a pixel stride of 2")

            // A buffer that is too short for its size is refused by the reader, not read past.
            val cut = QrReply.parse(QrNative.decode(w, h, stride, plane.copyOf(plane.size / 2)))
            ok(cut is QrReply.Refused && cut.code == "QR-10", "$name cut in half is refused with QR-10")
        }
        val blank = QrReply.parse(QrNative.decode(640, 480, 640, ByteArray(640 * 480) { 200.toByte() }))
        ok(blank is QrReply.Refused && blank.code == "QR-12", "a blank frame has no code: QR-12")
        val tiny = QrReply.parse(QrNative.decode(8, 8, 8, ByteArray(64)))
        ok(tiny is QrReply.Refused && tiny.code == "QR-10", "a frame of 8 by 8 is refused with QR-10")
        val negative = QrReply.parse(QrNative.decode(-5, 480, 640, ByteArray(640 * 480)))
        ok(negative is QrReply.Refused && negative.code == "QR-10", "a negative width is refused with QR-10")
        println("QrNativeHarness: $count checks passed")
    }
}
