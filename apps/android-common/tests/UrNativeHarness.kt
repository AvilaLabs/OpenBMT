package org.openbmt.app

import java.io.File
import org.openbmt.common.QrNative

/**
 * Loads the host-built `libopenbmt_qr_jni.so` and runs the UR call through the same Kotlin the apps use:
 * `UrCall` over `UrNative`, the animated code's parts and frames, and a reading of a sequence. The one
 * argument is the library path.
 */
object UrNativeHarness {
    private var count = 0

    private fun ok(cond: Boolean, what: String) {
        count++
        if (!cond) throw AssertionError("FAILED: $what")
    }

    private fun message(len: Int) = ByteArray(len) { (it * 7 + 3).toByte() }

    private fun psbt(len: Int) = "psbt".toByteArray() + byteArrayOf(0xff.toByte()) + message(len)

    private fun <T> value(o: UrOutcome<T>, what: String): T {
        ok(o is UrOutcome.Ok, "$what: ${(o as? UrOutcome.Refused)?.code ?: o}")
        return (o as UrOutcome.Ok).value
    }

    private fun refusal(o: UrOutcome<*>): String? = (o as? UrOutcome.Refused)?.code

    /** The frame the camera would give for [m]: scale 4 with a quiet zone of 4, as the reader's tests draw it. */
    private fun frame(m: QrMatrix): Triple<Int, Int, ByteArray> {
        val scale = 4
        val side = (m.side + 8) * scale
        val px = ByteArray(side * side) { 255.toByte() }
        for (y in 0 until m.side) for (x in 0 until m.side) {
            if (m.get(x, y)) {
                for (dy in 0 until scale) for (dx in 0 until scale) px[((y + 4) * scale + dy) * side + (x + 4) * scale + dx] = 0
            }
        }
        return Triple(side, side, px)
    }

    private fun qrTests(call: UrCall) {
        val m = value(call.qrEncode("HELLO".toByteArray(), 0), "a code of HELLO")
        ok(m.side == 21 && m.version == 1, "version 1 is 21 modules: ${m.side}")
        ok(m.get(0, 0) && m.get(6, 0) && !m.get(7, 0) && m.get(0, 6), "the finder pattern is where it should be")
        ok(!m.get(-1, 0) && !m.get(21, 0) && !m.get(0, 21), "outside the code is light")
        ok(refusal(call.qrEncode("x".toByteArray(), 4)) == UrCall.BAD_FRAMING, "a level outside 0 to 3")
        ok(refusal(call.qrEncode(ByteArray(3000) { 'a'.code.toByte() }, 3)) == "QR-01", "a text too long for any code")
        // What the library draws is what its reader reads.
        val (w, h, px) = frame(m)
        val read = QrReply.parse(QrNative.decode(w, h, w, px))
        ok(read is QrReply.Decoded && String(read.bytes) == "HELLO", "the drawn code reads back")
    }

    private fun partTests(call: UrCall) {
        val still = value(AnimatedQr.parts(call, "bytes", message(100)), "a short message")
        ok(still.size == 1 && still[0].startsWith("UR:BYTES/"), "a short message is one upper-case part")
        val frames = AnimatedFrames(call, still)
        ok(!frames.animated && frames.indexAt(0) == 0 && frames.indexAt(99_999) == 0, "a single part is a still code")
        val many = value(AnimatedQr.parts(call, "bytes", message(3000)), "a long message")
        ok(many.size > 2 && many.size % 2 == 0 && many.all { it == it.uppercase() && it.contains("-") }, "two turns of the sequence")
        ok(many[0].startsWith("UR:BYTES/1-${many.size / 2}/"), "the first part is 1 of the sequence length: ${many[0].take(20)}")
        ok(refusal(AnimatedQr.parts(call, "foo", message(10))) == UrCall.TYPE_NOT_USED, "another type")
        ok(refusal(AnimatedQr.parts(call, "crypto-psbt", message(10))) == "UR-14", "a plain PSBT must be one")
        ok(refusal(AnimatedQr.parts(call, "bytes", ByteArray(0))) == "UR-02", "an empty answer")
        ok(refusal(call.urEncode("bytes", message(10), 5)) == "UR-04", "a fragment under 10")
    }

    /** Every frame of either type draws as a code of version 12 or less, and the camera's reader reads it. */
    private fun versionTests(call: UrCall) {
        var parts = 0
        var highest = 0
        for (type in listOf("bytes", "crypto-psbt")) {
            for (len in listOf(5, 100, 229, 230, 231, 460, 1000, 5000, 20_000, 100_000)) {
                val payload = if (type == "bytes") message(len) else psbt(len)
                val list = value(AnimatedQr.parts(call, type, payload), "parts of $len bytes as $type")
                val frames = AnimatedFrames(call, list)
                val bad = frames.drawAll()
                ok(bad == null, "every part of $len bytes as $type draws (failed at $bad)")
                for (i in list.indices) {
                    val m = frames.matrix(i)!!
                    highest = maxOf(highest, m.version)
                    ok(m.version <= AnimatedQr.MAX_VERSION, "$type $len part $i: version ${m.version} is over ${AnimatedQr.MAX_VERSION}")
                }
                parts += list.size
                // One frame in a few, through the camera's reader.
                for (i in list.indices step maxOf(1, list.size / 3)) {
                    val (w, h, px) = frame(frames.matrix(i)!!)
                    val read = QrReply.parse(QrNative.decode(w, h, w, px))
                    ok(read is QrReply.Decoded && String(read.bytes) == list[i], "$type $len part $i reads back")
                }
            }
        }
        println("UrNativeHarness: $parts parts checked, the highest QR version is $highest (limit ${AnimatedQr.MAX_VERSION})")
    }

    private fun readingTests(call: UrCall) {
        val payload = message(2500)
        val parts = value(AnimatedQr.parts(call, "bytes", payload), "parts")
        val seq = parts.size / 2
        // Shuffled, with a fifth of the parts dropped and some repeated, half of them in lower case.
        val order = parts.indices.filter { it % 5 != 1 }.shuffled(java.util.Random(7)) + listOf(0, 0, 2)
        val reading = UrReading(call, setOf("bytes", "crypto-psbt"), Who.KEY_HOLDER)
        var done: UrStep.Done? = null
        var shown = 0
        for (i in order) {
            val text = if (i % 2 == 0) parts[i] else parts[i].lowercase()
            when (val step = reading.feed(text)) {
                is UrStep.Show -> {
                    shown++
                    ok(SayCheck.problem(step.words) == null, "the progress words pass: ${SayCheck.problem(step.words)}")
                    ok(step.words.what.contains("of about $seq parts"), "the words give the sequence length: ${step.words.what}")
                }
                is UrStep.Done -> {
                    done = step
                    break
                }
                else -> ok(false, "a good part gave $step")
            }
        }
        ok(done != null && done.type == "bytes" && done.payload.contentEquals(payload), "the sequence reads back to the payload")
        ok(shown >= 1, "progress was shown on the way")
        ok(reading.started, "the decoder is still open until it is dropped")
        reading.drop()
        ok(!reading.started, "dropped")
        reading.drop()

        // A whole UR is complete at once; a plain PSBT type is taken when the step takes it.
        val whole = value(AnimatedQr.parts(call, "crypto-psbt", psbt(40)), "a whole PSBT")
        val r2 = UrReading(call, setOf("crypto-psbt"), Who.MANAGER)
        val d2 = r2.feed(whole[0])
        ok(d2 is UrStep.Done && d2.type == "crypto-psbt" && d2.payload.contentEquals(psbt(40)), "a whole UR in one code")
        r2.drop()

        // A type the step does not take.
        val r3 = UrReading(call, setOf("bytes"), Who.MANAGER)
        val d3 = r3.feed(whole[0])
        ok(d3 is UrStep.Fail && SayCheck.problem(d3.words) == null && d3.words.heading == "Not the code expected", "the wrong type is refused with words")
        r3.drop()

        // Misreads are skipped.
        val r4 = UrReading(call, setOf("bytes"), Who.MANAGER)
        ok(r4.feed("hello") === UrStep.Ignore, "text that is not a UR is skipped")
        val swapped = parts[0].dropLast(1) + (if (parts[0].last() == 'A') 'B' else 'A')
        ok(r4.feed(swapped) === UrStep.Ignore, "a part with a changed letter is skipped")
        ok(r4.feed(parts[0]) is UrStep.Show, "and the reading goes on with a good one")
        // Parts of another stream in a row: skipped, and then the person is told.
        val other = value(AnimatedQr.parts(call, "bytes", message(2501)), "another stream")
        var told = 0
        for (i in 1..UrReading.STRANGER_RUN) {
            val s = r4.feed(other[1 + i % 3])
            if (s is UrStep.Show) {
                told++
                ok(SayCheck.problem(s.words) == null && s.words.heading == "Another code is in view", "the other-code words")
            } else {
                ok(s === UrStep.Ignore, "another stream's part is skipped")
            }
        }
        ok(told == 1, "told once after ${UrReading.STRANGER_RUN} in a row: $told")
        r4.drop()
    }

    private fun handleTests(call: UrCall) {
        val handles = (1..4).map { value(call.newDecoder(), "decoder $it") }
        ok(handles.toSet().size == 4, "four different handles")
        ok(refusal(call.newDecoder()) == UrCall.BAD_STATE, "a fifth decoder is refused")
        ok(call.drop(handles[0]) is UrOutcome.Ok, "one dropped")
        ok(refusal(call.drop(handles[0])) == UrCall.BAD_STATE, "dropped twice")
        ok(refusal(call.receive(handles[0], "ur:bytes/x")) == UrCall.BAD_STATE, "a dropped handle takes nothing")
        val fresh = value(call.newDecoder(), "a new decoder")
        ok(fresh !in handles, "a handle is never given twice: $fresh")
        ok(refusal(call.result(fresh)) == UrCall.BAD_STATE, "no result before the message is complete")
        for (h in handles.drop(1) + fresh) call.drop(h)
        ok(refusal(call.receive(fresh, "ur:bytes/x")) == UrCall.BAD_STATE, "all closed")
    }

    private fun framingTests(call: UrCall) {
        // The library's refusals of a request it cannot read come back framed as UR-21.
        val bad = byteArrayOf(1, 2, 3)
        val raw = org.openbmt.common.UrNative.call(bad)!!
        ok(raw.size == 11 && String(raw, 0, 4) == "OBMp" && raw[5].toInt() == 1 && String(raw, 6, 5) == "UR-21", "a request that is not framed gets a framed refusal")
        ok(org.openbmt.common.UrNative.call(ByteArray(0))!!.size == 11, "an empty request")
    }

    private fun hex(s: String) = ByteArray(s.length / 2) { s.substring(2 * it, 2 * it + 2).toInt(16).toByte() }

    private const val KEY_XPUB =
        "xpub6Ewx2N9hNSArJyF35CUGhaZLuZxQPNmJzWVwmpoV9U7Xu5wqka93nd3zEzokew9MzkNV4u6TCVDkHHR6QHQuYEFaasKzWkrkncXHMXGNdZP"

    /** The cosigner key of a code, through the library: a Keystone-layout account, a JSON export, and the refusals with words. */
    private fun keyTests(call: UrCall) {
        val account = hex(
            "a2011a56c4fac30281d90191d9012fa602f4035821" +
                "02d44daf959d7b3bf9343e2aea84ea21391ccf306a2765c35859c94cecb870aaea" +
                "045820f4ef1818131a780976e9dc1f30a4bd39903a9b8bbffad099035c49f886e85aed" +
                "05d90131a20100020006d90130a301881830f500f500f502f5021a56c4fac3030408" +
                "1ac07436a2",
        )
        val want = "[56c4fac3/48h/0h/0h/2h]$KEY_XPUB"
        val k = call.keyText("crypto-account", account)
        ok(k is KeyTextOutcome.Key && k.text == want, "a Keystone account gives the key text")
        // The JSON of a Coldcard-style export, read from an animated code as the apps read it.
        val json = "{\"xfp\": \"56C4FAC3\", \"p2wsh_deriv\": \"m/48'/0'/0'/2'\", \"p2wsh\": \"$KEY_XPUB\"}".toByteArray()
        val parts = value(AnimatedQr.parts(call, "bytes", json), "the parts of the export")
        val reading = UrReading(call, setOf("bytes"), Who.KEY_HOLDER)
        var done: UrStep.Done? = null
        for (p in parts) {
            val step = reading.feed(p)
            if (step is UrStep.Done) {
                done = step
                break
            }
        }
        reading.drop()
        ok(done != null && done.type == "bytes", "the export is read from its code")
        val j = call.keyText(done!!.type, done.payload)
        ok(j is KeyTextOutcome.Key && j.text == want, "the same key from the JSON")
        // Refusals come with words that pass the checks.
        val refused = listOf(
            Triple("hdkey", account, "UR-24"),
            Triple("account-descriptor", account, "UR-24"),
            Triple("crypto-output", account, "UR-24"),
            Triple("crypto-hdkey", hex("f6"), "UR-23"),
            Triple("crypto-account", account.copyOfRange(0, 40), "UR-23"),
            Triple("bytes", "not json".toByteArray(), "UR-29"),
            Triple("bytes", json.copyOf().also { it[0] = '['.code.toByte() }, "UR-29"),
        )
        val seen = ArrayList<Pair<String, SayWords>>()
        for ((type, payload, rule) in refused) {
            val d = call.keyText(type, payload)
            ok(d is KeyTextOutcome.Declined && d.code == rule, "$type is declined with $rule: ${(d as? KeyTextOutcome.Declined)?.code}")
            val say = UrWords.keyDeclined(Who.KEY_HOLDER, d as KeyTextOutcome.Declined)
            ok(SayCheck.problem(say) == null, "the words of $type pass: ${SayCheck.problem(say)}")
            seen.add("keyDeclined.$type.$rule" to say)
        }
        val long = call.keyText("x".repeat(65), account.copyOf())
        ok(long is KeyTextOutcome.Refused && long.code == "UR-01", "a type over 64 characters")
        ok(seen.map { it.second.heading }.toSet().size >= 4, "the refusals read differently")
    }

    private fun bbqrTests(call: UrCall) {
        val payload = psbt(900)
        val parts = value(call.bbqrEncode('P', payload, 300), "the parts of a BBQr set")
        ok(parts.size > 2 && parts.all { it.startsWith("B\$2P") && it.length <= 300 }, "a BBQr set of base32 parts of type P")
        val h = value(call.newJoiner(), "a reader")
        for ((i, p) in parts.reversed().withIndex()) {
            val progress = value(call.joinerAdd(h, p), "part $i")
            ok(progress.have == i + 1 && progress.total == parts.size && progress.complete == (i + 1 == parts.size), "progress at part $i")
        }
        val message = value(call.joinerResult(h), "the set's message")
        ok(message.kind == 'P' && message.payload.contentEquals(payload), "the set reads back in the reverse order")
        value(call.joinerDrop(h), "close the reader")
        ok(refusal(call.joinerDrop(h)) == UrCall.BAD_STATE, "a reader that is closed")
        val h2 = value(call.newJoiner(), "a second reader")
        value(call.joinerAdd(h2, parts[0]), "one part")
        ok(refusal(call.joinerResult(h2)) == "BQ-10", "a set that is short names the part missing")
        ok(refusal(call.joinerAdd(h2, "hello")) == "BQ-01", "text that is not a part")
        ok(refusal(call.bbqrEncode('Q', payload, 300)) == "BQ-03", "a kind that is not defined")
        value(call.joinerDrop(h2), "close the second reader")
        // A descriptor goes out as a crypto-output.
        val out = value(call.urEncode("crypto-output", message(300), 100), "the parts of a crypto-output")
        ok(out.size > 2 && out.all { it.startsWith("UR:CRYPTO-OUTPUT/") }, "a crypto-output of several parts")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        System.load(File(args[0]).absolutePath)
        val call = UrCall.native()
        qrTests(call)
        partTests(call)
        versionTests(call)
        readingTests(call)
        handleTests(call)
        framingTests(call)
        keyTests(call)
        bbqrTests(call)
        println("UrNativeHarness: $count checks passed")
    }
}
