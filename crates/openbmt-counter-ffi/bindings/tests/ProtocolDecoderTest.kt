package org.openbmt.counter

import java.io.ByteArrayOutputStream

/** Plain JVM regression harness for the production OBMn decoder and the OBMm encoder; run its main. */
object ProtocolDecoderTest {
    /** The `start` request of [sampleConfig], as the Rust test of the same name writes it. */
    private const val START_FRAME_HEX =
        "4f424d6d010100000000000000000001000000640100000000000000010000000201010000007001010000006f010100000003"

    /** The `ln_reply` request for handle 1, as the Rust test of the same name writes it. */
    private const val LN_REPLY_FRAME_HEX =
        "4f424d6d01330001000000000000000700000000000000c8000000" + "020000007b7d"

    /** The `rate_parse` and `rate_combine` requests, as the Rust test of the same name writes them. */
    private const val RATE_PARSE_HEX =
        "4f424d6d0129000000000000000000060000006b72616b656e020000007b7d0900000000000000"
    private const val RATE_COMBINE_HEX =
        "4f424d6d012a00000000000000000002000000060000006b72616b656e01000000000000003a3c70" +
            "000000000008000000636f696e626173650200000000000000523b70000000000003000000000000" +
            "00"

    @JvmStatic
    fun main(args: Array<String>) {
        val success = decodeResponse(header(0))
        success.end()

        val error = decodeError(errorFrame())
        check(error.rule == "sample-rule")
        check(error.text == "sample text")
        check(error.status == 1)
        check(error.words?.heading == "Heading")
        check(error.words?.who == 5)

        rejects("status") { decodeResponse(errorFrame(status = 5)) }
        rejects("tone") { decodeResponse(errorFrame(tone = 4)) }
        rejects("person") { decodeResponse(errorFrame(who = 7)) }
        rejects("UTF-8") { decodeResponse(errorFrame(heading = byteArrayOf(0xff.toByte()))) }
        rejects("truncation") { decodeResponse(errorFrame().copyOf(errorFrame().size - 1)) }
        rejects("trailing bytes") { decodeResponse(errorFrame(trailingByte = true)) }
        rejects("the Key app's magic", "response-magic") {
            decodeResponse(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), '9'.code.toByte(), 1, 0))
        }

        screens()
        requests()
        rates()
    }

    // ---- the price ops ----

    private fun rates() {
        val sources = decodeResponse(
            header(0, Writer().u32(1).string("kraken").string("https://k.example/").string("USD").string("k").toByteArray()),
        ).let { r -> r.list { r.rateSource() }.also { r.end() } }
        check(sources.size == 1 && sources[0].id == "kraken" && sources[0].initial == "k" && sources[0].currency == "USD")
        val sample = decodeResponse(header(0, Writer().string("kraken").u64(9).u64(7_355_450).toByteArray()))
            .let { r -> r.sample().also { r.end() } }
        check(sample == Sample("kraken", 9, 7_355_450))
        val rate = decodeResponse(header(0, Writer().u64(7_355_218).string("med.kc").u64(5).toByteArray()))
            .let { r -> r.rate().also { r.end() } }
        check(rate.rate == 7_355_218L && rate.sourceLabel == "med.kc" && rate.sourceFetchedAt == 5L)
        val shop = decodeResponse(header(0, Writer().u32(3).u8(1).u8(0).u64(26_000).toByteArray()))
            .let { r -> r.shopSettings().also { r.end() } }
        check(shop.version == 3L && shop.torOn && !shop.customerWifi && shop.minSaleSats == 26_000L)
        rejects("a bool in the settings") {
            decodeResponse(header(0, Writer().u32(3).u8(2).u8(0).u64(1).toByteArray())).shopSettings()
        }
        rejects("trailing bytes in a rate") {
            decodeResponse(header(0, Writer().u64(1).string("a").u64(5).u8(0).toByteArray())).let { it.rate(); it.end() }
        }
        check(hex(rateParseFrame("kraken", byteArrayOf(0x7b, 0x7d), 9)) == RATE_PARSE_HEX) { hex(rateParseFrame("kraken", byteArrayOf(0x7b, 0x7d), 9)) }
        val samples = listOf(Sample("kraken", 1, 7_355_450), Sample("coinbase", 2, 7_355_218))
        check(hex(rateCombineFrame(samples, 3)) == RATE_COMBINE_HEX) { hex(rateCombineFrame(samples, 3)) }
        rejects("the refusal of a rate", "RT-37") {
            throw CounterException("RT-37", "no price is fresh enough")
        }
    }

    // ---- the results ----

    private fun screens() {
        val screen = decodeResponse(screenFrame()).screen()
        check(screen.state == SaleState.Waiting)
        check(screen.words.heading == "Waiting" && screen.words.tone == 0 && screen.words.who == 2)
        check(screen.flags.size == 1 && screen.flags[0].heading == "Slow")
        check(screen.receivedSats == 5000L && screen.needsSats == 6000L)
        check(screen.stuck && !screen.late && screen.reopened && !screen.conflictSeen)
        check(
            screen.reports == listOf<Report>(
                Report.PosPaid(7),
                Report.BooksFinal(1, 500, "USD", 5000),
                Report.Dust(2, 9),
            ),
        )
        val payment = screen.payment ?: error("a payment")
        check(payment.address == "bc1qaddress" && payment.uri == "BITCOIN:BC1QADDRESS")
        check(payment.copyUri == "bitcoin:bc1qaddress" && payment.qrSize == 2L)
        // Bits 1, 0, 1, 0: the first column is dark.
        check(payment.module(0, 0) && !payment.module(1, 0) && payment.module(0, 1) && !payment.module(1, 1))

        check(screen.check == null && screen.lightning == null)
        decodeResponse(screenFrame(lightning = 1)).screen().also {
            val ln = it.lightning ?: error("a Lightning line")
            check(ln.invoice && !ln.waiting && ln.unavailable == null)
        }
        decodeResponse(screenFrame(lightning = 2)).screen().also {
            val ln = it.lightning ?: error("a Lightning line")
            check(!ln.invoice && !ln.waiting && ln.unavailable?.heading == "Not available")
        }
        rejects("a bool in the Lightning line") { decodeResponse(screenFrame(lightning = 3)).screen() }
        decodeResponse(screenFrame(check = true)).screen().also { check(it.check?.heading == "Light check") }
        decodeResponse(screenFrame(payment = false)).screen().also { check(it.payment == null) }
        decodeResponse(screenFrame(allReports = true)).screen().also { check(it.reports.size == 7) }
        rejects("a state tag") { decodeResponse(screenFrame(state = 12)).screen() }
        decodeResponse(screenFrame(state = 11)).screen().also { check(it.state == SaleState.PaidLightning) }
        rejects("a report tag") { decodeResponse(screenFrame(reportTag = 7)).screen() }
        rejects("the size of the modules") { decodeResponse(screenFrame(bitsLength = 2)).screen() }
        rejects("a bool") { decodeResponse(screenFrame(stuck = 2)).screen() }
        rejects("trailing bytes in a screen") {
            decodeResponse(screenFrame(trailingByte = true)).also { it.screen(); it.end() }
        }

        val pump = decodeResponse(header(0, Writer().u32(0).u32(1).string("note").u8(1).toByteArray())).pump()
        check(pump.reports.isEmpty() && pump.notes == listOf("note") && pump.stopped)
        statuses()
    }

    /** The status with and without the own node's line (CR-107). */
    private fun statuses() {
        fun status(own: Boolean, flag: Int = if (own) 1 else 0, lightning: Boolean = false): ByteArray {
            val out = ByteArrayOutputStream()
            out.write(Writer().u32(5).u32(6).toByteArray())
            out.write(0) // no health answer yet
            out.write(flag)
            if (own) {
                out.write(0) // not usable
                words(out, 0, 0, "Your node is not reachable".toByteArray(Charsets.UTF_8))
            }
            out.write(Writer().u32(1).u32(0).u64(0).u8(0).toByteArray())
            if (lightning) {
                out.write(1)
                out.write(1) // ready
                words(out, 1, 0, "Lightning ready".toByteArray(Charsets.UTF_8))
            } else {
                out.write(0)
            }
            return header(0, out.toByteArray())
        }
        decodeResponse(status(false)).let { r ->
            val s = r.statusView()
            r.end()
            check(s.checkedHeight == 5L && s.tipHeight == 6L && s.health == null && s.ownNode == null)
            check(s.sales == listOf(0L) && !s.stopped && s.lightning == null)
        }
        decodeResponse(status(false, lightning = true)).let { r ->
            val s = r.statusView()
            r.end()
            val ln = s.lightning ?: error("a Lightning line")
            check(ln.ready && ln.words.heading == "Lightning ready")
        }
        decodeResponse(status(true)).let { r ->
            val s = r.statusView()
            r.end()
            val own = s.ownNode ?: error("an own node line")
            check(!own.usable && own.words.heading == "Your node is not reachable")
        }
        rejects("an option flag in the status") { decodeResponse(status(false, flag = 2)).statusView() }
    }

    // ---- the requests ----

    private fun requests() {
        val frame = requestFrame(12, 1) { it.u64(1_700_000_000).u32(5) }
        check(
            hex(frame) == "4f424d6d010c000100000000000000" + "00f1536500000000" + "05000000",
        ) { hex(frame) }
        val config =
            StartConfig(
                dir = "d",
                now = 1,
                record = byteArrayOf(2),
                proxy = "p",
                ownNode = "o",
                lightning = byteArrayOf(3),
            )
        check(hex(startFrame(config)) == START_FRAME_HEX) { hex(startFrame(config)) }
        val reply = lnReplyFrame(1, 7, 200, byteArrayOf(0x7b, 0x7d))
        check(hex(reply) == LN_REPLY_FRAME_HEX) { hex(reply) }
        lightning()
    }

    /** The results of the Lightning ops. */
    private fun lightning() {
        val request = Writer().u64(9).string("node.local:8080").u8(1).string("/v1/invoices")
            .string("Rune").string("secret").u8(1).string("{}").toByteArray()
        decodeResponse(header(0, Writer().u8(1).raw(request).u8(0).toByteArray())).let { r ->
            val step = r.lnStep()
            r.end()
            val next = step.next ?: error("a request")
            check(next.id == 9L && next.hostPort == "node.local:8080" && next.method == LnMethod.Post)
            check(next.path == "/v1/invoices" && next.headerName == "Rune" && next.body == "{}")
            check(!next.toString().contains("secret"))
            check(step.record == null)
        }
        decodeResponse(header(0, Writer().u8(0).u8(1).bytes(byteArrayOf(1, 2, 3)).toByteArray())).let { r ->
            val step = r.lnStep()
            r.end()
            check(step.next == null && step.record!!.contentEquals(byteArrayOf(1, 2, 3)))
        }
        rejects("a request method") {
            val bad = Writer().u64(9).string("h:1").u8(2).string("/").string("n").string("v").u8(0).toByteArray()
            decodeResponse(header(0, bad)).lnRequest()
        }
    }

    private fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    // ---- frames ----

    private fun header(status: Int, rest: ByteArray = ByteArray(0)): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'n'.code.toByte()))
        out.write(1)
        out.write(status)
        out.write(rest)
        return out.toByteArray()
    }

    private fun decodeError(frame: ByteArray): CounterException = try {
        decodeResponse(frame)
        error("expected an OBMn error")
    } catch (e: CounterException) {
        e
    }

    private inline fun rejects(label: String, rule: String = "response-malformed", block: () -> Unit) {
        val e = try {
            block()
            error("accepted invalid $label")
        } catch (e: CounterException) {
            e
        }
        check(e.rule == rule) { "$label: ${e.rule}" }
    }

    private fun string16(out: ByteArrayOutputStream, bytes: ByteArray) {
        out.write(bytes.size and 0xff)
        out.write((bytes.size ushr 8) and 0xff)
        out.write(bytes)
    }

    private fun words(
        out: ByteArrayOutputStream,
        tone: Int,
        who: Int,
        heading: ByteArray,
    ) {
        out.write(tone)
        out.write(who)
        string16(out, heading)
        string16(out, "What happened.".toByteArray(Charsets.UTF_8))
        string16(out, "Try again.".toByteArray(Charsets.UTF_8))
        out.write(0) // code absent
        out.write(0) // at absent
        out.write(0) // quoted absent
        out.write(0) // detail absent
    }

    private fun errorFrame(
        status: Int = 1,
        tone: Int = 0,
        who: Int = 5,
        heading: ByteArray = "Heading".toByteArray(Charsets.UTF_8),
        trailingByte: Boolean = false,
    ): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'n'.code.toByte()))
        out.write(1)
        out.write(status)
        string16(out, "sample-rule".toByteArray(Charsets.UTF_8))
        string16(out, "sample text".toByteArray(Charsets.UTF_8))
        words(out, tone, who, heading)
        if (trailingByte) out.write(0x55)
        return out.toByteArray()
    }

    private fun screenFrame(
        state: Int = 2,
        payment: Boolean = true,
        allReports: Boolean = false,
        reportTag: Int = -1,
        bitsLength: Int = 1,
        stuck: Int = 1,
        trailingByte: Boolean = false,
        check: Boolean = false,
        lightning: Int = 0,
    ): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(state)
        words(out, 0, 2, "Waiting".toByteArray(Charsets.UTF_8))
        // One extra line.
        out.write(byteArrayOf(1, 0, 0, 0))
        words(out, 0, 2, "Slow".toByteArray(Charsets.UTF_8))
        // The check line, when asked for.
        if (check) {
            out.write(1)
            words(out, 0, 0, "Light check".toByteArray(Charsets.UTF_8))
        } else {
            out.write(0)
        }
        out.write(Writer().u64(5000).u64(6000).toByteArray())
        out.write(stuck) // stuck
        out.write(0) // late
        out.write(1) // reopened
        out.write(0) // conflict seen
        val reports = Writer()
        if (allReports) {
            reports.u32(7)
            reports.u8(0).u32(1)
            reports.u8(1).u32(2)
            reports.u8(2).u32(3).u64(4).string("EUR").u64(5)
            reports.u8(3).u32(4).u64(5)
            reports.u8(4).u32(5).u64(6)
            reports.u8(5).u32(6)
            reports.u8(6).u32(7).u64(8)
        } else if (reportTag >= 0) {
            reports.u32(1).u8(reportTag).u32(1)
        } else {
            reports.u32(3)
            reports.u8(0).u32(7)
            reports.u8(2).u32(1).u64(500).string("USD").u64(5000)
            reports.u8(6).u32(2).u64(9)
        }
        out.write(reports.toByteArray())
        if (payment) {
            out.write(1)
            val p = Writer().string("bc1qaddress").string("BITCOIN:BC1QADDRESS").string("bitcoin:bc1qaddress")
                .u32(2).bytes(ByteArray(bitsLength) { 0xa0.toByte() })
            out.write(p.toByteArray())
        } else {
            out.write(0)
        }
        when (lightning) {
            0 -> out.write(0)
            1 -> out.write(byteArrayOf(1, 1, 0, 0))
            2 -> {
                out.write(byteArrayOf(1, 0, 0, 1))
                words(out, 2, 2, "Not available".toByteArray(Charsets.UTF_8))
            }
            else -> out.write(byteArrayOf(1, 2, 0, 0))
        }
        if (trailingByte) out.write(0x55)
        return header(0, out.toByteArray())
    }
}
