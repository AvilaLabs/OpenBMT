package org.openbmt.counter

import java.io.ByteArrayOutputStream

/** Plain JVM regression harness for the production OBMn decoder and the OBMm encoder; run its main. */
object ProtocolDecoderTest {
    /** The `start` request of [sampleConfig], as the Rust test of the same name writes it. */
    private const val START_FRAME_HEX =
        "4f424d6d010100000000000000000001000000640100000000000000010000000201010000007001010000006f01010000000300"

    /** The `ln_reply` request for handle 1, as the Rust test of the same name writes it. */
    private const val LN_REPLY_FRAME_HEX =
        "4f424d6d01330001000000000000000700000000000000c8000000" + "020000007b7d"

    /** The `rate_parse` and `rate_combine` requests, as the Rust test of the same name writes them. */
    private const val RATE_PARSE_HEX =
        "4f424d6d0129000000000000000000060000006b72616b656e020000007b7d0900000000000000"
    private const val TAP_ANSWER_HEX = "4f424d6d01590000000000000000000000000001030000000001020400000000a40400"
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
        days()
        requestsToSend()
        bills()
        till()
    }

    // ---- bills, tabs and the till's reference (CR-245 to CR-254) ----

    private fun wordsBytes(): ByteArray {
        val out = ByteArrayOutputStream()
        words(out, 0, 1, "WORDS_NEEDED_X".toByteArray(Charsets.UTF_8))
        return out.toByteArray()
    }

    private fun bills() {
        // open_bill (74): the time, the kind (1 bill, 2 tab), the amount and the reference.
        val open = requestFrame(74, 1) { it.u64(7).u8(2).u64(1_200).option("T-14") { x -> it.string(x) } }
        check(hex(open).endsWith("0700000000000000" + "02" + "b004000000000000" + "01" + "04000000" + "542d3134")) { hex(open) }
        // add_other_part (78) and close_short (82).
        check(hex(requestFrame(78, 1) { it.u64(9).u32(3).u64(500).u8(2) }).endsWith("0900000000000000" + "03000000" + "f401000000000000" + "02"))
        check(hex(requestFrame(82, 1) { it.u64(9).u32(3).u8(1).string("Ana") }).endsWith("0900000000000000" + "03000000" + "01" + "03000000" + "416e61"))
        // bill_split (79) takes the bill and the number of people.
        check(hex(requestFrame(79, 1) { it.u32(3).u8(4) }).endsWith("03000000" + "04"))
        // bills (84): two bills, one open with a bitcoin part and one closed short.
        val w = Writer().u32(2)
        w.u32(1).u8(1).u64(1_000).u8(1).string("T-1").u64(100).u8(0).u64(0).u64(0).u64(0).u64(300).u32(1).u64(700)
        w.u32(1).u8(1).u32(0).u64(500).u8(0).u8(0)
        w.raw(wordsBytes())
        w.u32(2).u8(2).u64(900).u8(0).u64(150).u8(2).u64(200).u64(300).u64(0).u64(200).u32(0).u64(400)
        w.u32(1).u8(0).u64(200).u8(1).u8(2).u8(1)
        w.raw(wordsBytes())
        decodeResponse(header(0, w.toByteArray())).let { r ->
            val list = r.list { r.bill() }
            r.end()
            check(list.size == 2 && list[0].number == 1L && list[0].kind == BillKind.Bill && list[0].reference == "T-1")
            check(list[0].state == BillState.Open && list[0].pending == 1L && list[0].left == 700L)
            check(list[0].parts.size == 1 && list[0].parts[0].sale == 0L && !list[0].parts[0].counts)
            check(list[1].kind == BillKind.Tab && list[1].reference == null && list[1].state == BillState.ClosedShort)
            check(list[1].unpaidMinor == 300L && list[1].endedAt == 200L)
            check(list[1].parts[0].sale == null && list[1].parts[0].way == PartWay.Card && list[1].parts[0].counts)
        }
        rejects("a kind that is not 1 or 2 in a bill") {
            decodeResponse(header(0, Writer().u32(1).u32(1).u8(9).toByteArray())).let { r -> r.list { r.bill() } }
        }
        rejects("a state that is not 0 to 3 in a bill") {
            decodeResponse(header(0, Writer().u32(1).u32(1).u8(1).u64(1).u8(0).u64(0).u8(9).toByteArray())).let { r -> r.list { r.bill() } }
        }
        rejects("a way that is not 1 to 3 in a part") {
            val p = Writer().u32(1).u32(1).u8(1).u64(1).u8(0).u64(0).u8(0).u64(0).u64(0).u64(0).u64(0).u32(0).u64(0)
            p.u32(1).u8(0).u64(5).u8(1).u8(9).u8(1)
            decodeResponse(header(0, p.toByteArray())).let { r -> r.list { r.bill() } }
        }
    }

    // ---- a sale from the till (CR-266 to CR-271) ----

    private fun till() {
        val sale = ByteArray(16) { 4 }
        // till_start (86): the time, the till's sale as bytes, the rate, the source, when it was fetched and no person.
        val start = requestFrame(86, 1) { it.u64(7).bytes(sale).u64(10_000_000).string("test-fixed").u64(5).option(null as Actor?) { } }
        check(
            hex(start).endsWith(
                "0700000000000000" + "10000000" + "04".repeat(16) + "8096980000000000" + "0a000000" + "746573742d6669786564" +
                    "0500000000000000" + "00",
            ),
        ) { hex(start) }
        // till_not_now (87): the time and the till's sale.
        check(hex(requestFrame(87, 1) { it.u64(9).bytes(sale) }).endsWith("0900000000000000" + "10000000" + "04".repeat(16)))
        // till_waiting (85): one request with 5 minutes left.
        val w = Writer().u32(1).bytes(sale).u64(500).string("POS/0007").u64(1_300).u64(5)
        w.raw(wordsBytes())
        decodeResponse(header(0, w.toByteArray())).let { r ->
            val list = r.list { r.tillRequest() }
            r.end()
            check(list.size == 1 && list[0].sale.contentEquals(sale) && list[0].amountMinor == 500L)
            check(list[0].reference == "POS/0007" && list[0].lapsesAt == 1_300L && list[0].minutesLeft == 5L)
            check(list[0].words.heading == "WORDS_NEEDED_X")
        }
        // till_of_sale (88): a sale the till cancelled, with its line; and a sale that did not come from the till.
        val cancelled = Writer().u8(1).bytes(sale).u8(1).u8(1).raw(wordsBytes())
        decodeResponse(header(0, cancelled.toByteArray())).let { r ->
            val v = r.tillOfSale()
            r.end()
            check(v.sale!!.contentEquals(sale) && v.cancelled && v.words?.heading == "WORDS_NEEDED_X")
        }
        decodeResponse(header(0, Writer().u8(0).u8(0).u8(0).toByteArray())).let { r ->
            val v = r.tillOfSale()
            r.end()
            check(v.sale == null && !v.cancelled && v.words == null)
        }
        rejects("a flag that is not 0 or 1 in a sale of the till") {
            decodeResponse(header(0, Writer().u8(0).u8(7).u8(0).toByteArray())).let { r -> r.tillOfSale() }
        }
    }

    // ---- requests to send and notes (CR-205, CR-206, CR-215) ----

    private fun requestsToSend() {
        // open_request (61): the fields of `open`, then the note and the days.
        val frame =
            requestFrame(61, 1) {
                it.u64(7).u64(500).string("USD").u64(10_000_000).string("src").u64(7).u8(0)
                    .option("Tour 12 May") { n -> it.string(n) }.u8(3)
            }
        check(
            hex(frame) ==
                "4f424d6d013d000100000000000000" + "0700000000000000" + "f401000000000000" + "03000000555344" +
                    "8096980000000000" + "03000000737263" + "0700000000000000" + "00" + "01" +
                    "0b000000" + "546f7572203132204d6179" + "03",
        ) { hex(frame) }
        // add_note (62) and request_link (65).
        check(hex(requestFrame(62, 1) { it.u64(9).u32(2).string("hi") }).endsWith("0900000000000000" + "02000000" + "02000000" + "6869"))
        check(hex(requestFrame(65, 1) { it.u32(2).string("l") }).endsWith("02000000" + "01000000" + "6c"))
        // sale_info (63): a note and the days, and none.
        decodeResponse(header(0, Writer().u8(1).string("Tour").u8(7).u64(1_700_604_800).u8(1).u8(1).string("T-7").u8(1).u32(4).toByteArray())).let { r ->
            val info = r.saleInfo()
            r.end()
            check(info.note == "Tour" && info.requestDays == 7 && info.expiresAt == 1_700_604_800L && info.cashRefundOnly)
            check(info.reference == "T-7" && info.billNumber == 4L)
        }
        decodeResponse(header(0, Writer().u8(0).u8(0).u64(900).u8(0).u8(0).u8(0).toByteArray())).let { r ->
            val info = r.saleInfo()
            r.end()
            check(info.note == null && info.requestDays == 0 && info.expiresAt == 900L && !info.cashRefundOnly)
            check(info.reference == null && info.billNumber == null)
        }
        rejects("a flag that is not 0 or 1 in a sale's info") {
            decodeResponse(header(0, Writer().u8(2).u8(0).u64(900).u8(0).toByteArray())).saleInfo()
        }
        rejects("a cut in a sale's info") {
            decodeResponse(header(0, Writer().u8(0).u8(0).toByteArray())).saleInfo()
        }
        // requests (64): newest first, as the library lists them.
        val rows = Writer().u32(2)
        rows.u32(5).u8(1).string("Tour").u64(500).u64(100).u64(200).u64(5_000)
        rows.u32(4).u8(0).u64(700).u64(50).u64(60).u64(7_000)
        decodeResponse(header(0, rows.toByteArray())).let { r ->
            val list = r.list { r.openRequest() }
            r.end()
            check(list.size == 2 && list[0].index == 5L && list[0].note == "Tour" && list[0].needsSats == 5_000L)
            check(list[1].index == 4L && list[1].note == null && list[1].amountMinor == 700L)
            check(list[1].openedAt == 50L && list[1].expiresAt == 60L)
        }
        rejects("trailing bytes after the requests") {
            decodeResponse(header(0, Writer().u32(0).u8(0).toByteArray())).let { r -> r.list { r.openRequest() }; r.end() }
        }
    }

    // ---- the day's totals (CR-153) ----

    private fun tally(w: Writer, count: Long, minor: Long, sats: Long): Writer = w.u32(count).u64(minor).u64(sats)

    private fun dayFrame(role: Int = 1, trailing: Boolean = false, currency: Boolean = true): ByteArray {
        val w = Writer().u64(1_700_000_000).u64(1_700_086_400)
        if (currency) w.u8(1).string("USD") else w.u8(0)
        tally(w, 3, 4500, 54_000)
        tally(w, 2, 3000, 36_000)
        tally(w, 1, 1500, 18_000)
        tally(w, 2, 2000, 24_000)
        w.u32(1).u32(4)
        tally(w, 1, 0, 12_000)
        tally(w, 2, 0, 5_000)
        w.u32(1)
        tally(w, 1, 500, 6_000)
        w.u32(2).u64(300)
        w.u32(2)
        w.u32(1).u8(role).string("Ann").u32(4).u32(1).u32(0).u32(1).u64(150)
        w.u32(3)
        if (trailing) w.u8(0)
        return header(0, w.toByteArray())
    }

    private fun days() {
        val day = decodeResponse(dayFrame()).let { r -> r.dayTotals().also { r.end() } }
        check(day.from == 1_700_000_000L && day.to == 1_700_086_400L && day.currency == "USD")
        check(day.paid.count == 3L && day.paid.minor == 4500L && day.paid.sats == 54_000L)
        check(day.onChain.count == 2L && day.lightning.count == 1L && day.waiting.sats == 24_000L)
        check(day.stopped == 1L && day.ended == 4L && day.overrides.sats == 12_000L && day.refunds.count == 2L)
        check(day.refundsWithdrawn == 1L && day.cashRefunds.minor == 500L && day.unnamedSales == 2L)
        check(day.people.size == 1 && day.people[0].role == ActorRole.Manager && day.people[0].name == "Ann")
        check(day.people[0].sales == 4L && day.people[0].cashRefunds == 1L && day.peopleLeftOut == 3L)
        check(day.tips.count == 2L && day.tips.minor == 300L && day.people[0].tipsMinor == 150L)
        check(decodeResponse(dayFrame(role = 0, currency = false)).let { r -> r.dayTotals().also { r.end() } }.currency == null)
        rejects("a role that is not known in a person's line") { decodeResponse(dayFrame(role = 2)).dayTotals() }
        rejects("trailing bytes after the totals") { decodeResponse(dayFrame(trailing = true)).let { it.dayTotals(); it.end() } }
        rejects("a cut in the totals") { decodeResponse(dayFrame().copyOf(dayFrame().size - 1)).dayTotals() }
        // The `cancel` request carries the person at the till as an option: a flag, then the role and the name.
        val noOne = requestFrame(14, 1) { it.u64(5).u32(2).option<Actor>(null) { } }
        check(hex(noOne) == "4f424d6d010e000100000000000000" + "0500000000000000" + "02000000" + "00") { hex(noOne) }
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
        val shop = decodeResponse(header(0, Writer().u32(3).u8(1).u8(0).u64(26_000).string("EUR").u8(1).bytes(byteArrayOf(5, 18)).toByteArray()))
            .let { r -> r.shopSettings().also { r.end() } }
        check(shop.version == 3L && shop.torOn && !shop.customerWifi && shop.minSaleSats == 26_000L && shop.currency == "EUR")
        check(shop.tipsOn && shop.tipPercents == listOf(5, 18))
        rejects("a bool in the settings") {
            decodeResponse(header(0, Writer().u32(3).u8(2).u8(0).u64(1).string("USD").u8(0).bytes(byteArrayOf()).toByteArray())).shopSettings()
        }
        rejects("trailing bytes in a rate") {
            decodeResponse(header(0, Writer().u64(1).string("a").u64(5).u8(0).toByteArray())).let { it.rate(); it.end() }
        }
        check(hex(tapAnswerFrame(byteArrayOf(), byteArrayOf(0, 1, 2), byteArrayOf(0, 0xa4.toByte(), 4, 0))) == TAP_ANSWER_HEX)
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
        check(payment.tag.contentEquals(byteArrayOf(0, 3, 0xd1.toByte())))
        // Bits 1, 0, 1, 0: the first column is dark.
        check(payment.module(0, 0) && !payment.module(1, 0) && payment.module(0, 1) && !payment.module(1, 1))

        check(screen.check == null && screen.lightning == null)
        decodeResponse(screenFrame(lightning = 1)).screen().also {
            val ln = it.lightning ?: error("a Lightning line")
            check(ln.invoice && !ln.waiting && ln.unavailable == null && !ln.notForThisSale && ln.failed == null)
        }
        decodeResponse(screenFrame(lightning = 2)).screen().also {
            val ln = it.lightning ?: error("a Lightning line")
            check(!ln.invoice && !ln.waiting && ln.unavailable?.heading == "Not available")
        }
        decodeResponse(screenFrame(lightning = 4)).screen().also {
            val ln = it.lightning ?: error("a Lightning line")
            check(!ln.invoice && ln.notForThisSale && ln.failed == null && ln.unavailable == null)
        }
        decodeResponse(screenFrame(lightning = 5)).screen().also {
            val ln = it.lightning ?: error("a Lightning line")
            check(!ln.notForThisSale && ln.failed?.heading == "Not finished")
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
        links()
    }

    /** The link's view, with and without a time (CR-136), and the pairing view with a relay. */
    private fun links() {
        fun link(since: Boolean, flag: Int = if (since) 1 else 0): ByteArray {
            val out = ByteArrayOutputStream()
            words(out, 0, 0, "Box not reachable".toByteArray(Charsets.UTF_8))
            out.write(0) // not connected
            out.write(flag)
            if (since) out.write(Writer().u64(1_700_000_000).toByteArray())
            out.write(1) // refused
            out.write(Writer().u64(3).u64(7).toByteArray())
            out.write(0) // no check-in yet
            out.write(Writer().u32(2).toByteArray()) // settings version
            out.write(0) // no restart needed
            out.write(0) // no settings line
            return out.toByteArray()
        }
        decodeResponse(header(0, link(true))).let { r ->
            val l = r.linkView()
            r.end()
            check(l.words.heading == "Box not reachable" && !l.connected && l.since == 1_700_000_000L)
            check(l.refused && l.heartbeatsTaken == 3L && l.ownerRecords == 7L && l.lastCheckInAt == null)
            check(l.settingsVersion == 2L && !l.restartNeeded && l.settingsLine == null)
        }
        decodeResponse(header(0, link(false))).let { r ->
            check(r.linkView().since == null)
            r.end()
        }
        rejects("an option flag in the link") { decodeResponse(header(0, link(false, flag = 2))).linkView() }
        val step = Writer().u32(1).string("a note").toByteArray()
        decodeResponse(header(0, link(true) + step)).let { r ->
            val s = r.linkStepView()
            r.end()
            check(s.notes == listOf("a note"))
        }
        // The pairing view ends with the kind of relay; a tag that is not defined is refused.
        fun relay(tag: Int) = Writer().u8(3).u32(1).u8(0).u8(0).u64(0).string("USD").u8(0).bytes(byteArrayOf(10, 15, 20)).u8(tag).toByteArray()
        decodeResponse(header(0, relay(2))).let { r ->
            r.u8()
            r.shopSettings()
            check(r.relayKind() == RelayKind.Onion)
            r.end()
        }
        rejects("a relay kind") {
            decodeResponse(header(0, relay(3))).let { r ->
                r.u8()
                r.shopSettings()
                r.relayKind()
            }
        }
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
                linkProxy = null,
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
        // CR-225: a request for the swap provider is an id, a host and port and the bytes to send.
        decodeResponse(header(0, Writer().u8(1).u64(4).string("abc.onion:80").bytes(byteArrayOf(71, 69, 84)).toByteArray())).let { r ->
            val next = r.option { r.swapRequest() }
            r.end()
            check(next != null && next.id == 4L && next.hostPort == "abc.onion:80" && next.bytes.contentEquals(byteArrayOf(71, 69, 84)))
        }
        decodeResponse(header(0, Writer().u8(0).toByteArray())).let { r ->
            check(r.option { r.swapRequest() } == null)
            r.end()
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
                .u32(2).bytes(ByteArray(bitsLength) { 0xa0.toByte() }).bytes(byteArrayOf(0, 3, 0xd1.toByte()))
            out.write(p.toByteArray())
        } else {
            out.write(0)
        }
        when (lightning) {
            0 -> out.write(0)
            1 -> out.write(byteArrayOf(1, 1, 0, 0, 0, 0))
            2 -> {
                out.write(byteArrayOf(1, 0, 0, 1))
                words(out, 2, 2, "Not available".toByteArray(Charsets.UTF_8))
                out.write(byteArrayOf(0, 0))
            }
            4 -> out.write(byteArrayOf(1, 0, 0, 0, 1, 0))
            5 -> {
                out.write(byteArrayOf(1, 0, 0, 0, 0, 1))
                words(out, 2, 2, "Not finished".toByteArray(Charsets.UTF_8))
            }
            else -> out.write(byteArrayOf(1, 2, 0, 0, 0, 0))
        }
        if (trailingByte) out.write(0x55)
        return header(0, out.toByteArray())
    }
}
