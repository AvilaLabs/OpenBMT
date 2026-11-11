package org.openbmt.owner

import java.io.ByteArrayOutputStream

/** Plain JVM regression harness for the production OBMl decoder and the OBMk encoder; run its main. */
object ProtocolDecoderTest {
    /** The `start` request of [sampleConfig], as the Rust test of the same name writes it. */
    private const val START_FRAME_HEX =
        "4f424d6b01010000000000000000000100000064030100000000000000010000000101000000020100000077010000000300040000000500000006000000010100000070010000000100000061000101000100000078010000000701000000010000000809000000000000000a00000000000000" +
            ""

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
        rejects("the counter app's magic", "response-magic") {
            decodeResponse(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'n'.code.toByte(), 1, 0))
        }

        results()
        requests()
    }

    // ---- the results ----

    private fun results() {
        val status = decodeResponse(statusFrame()).statusView()
        check(status.view == ViewKind.Ready && status.rescanTo == null)
        check(status.viewWords.heading == "Words" && status.receiptsWords.who == 4)
        check(status.checkedHeight == 6L && status.tipHeight == 7L)
        check(status.halted == null && status.recoveryWords == null)
        check(status.health.known && status.health.healthy && !status.health.behind)
        check(status.balanceSats == 300_000L && !status.recoveryPending && !status.stopped)
        rejects("a view tag") { decodeResponse(statusFrame(view = 3)).statusView() }
        rejects("a bool") { decodeResponse(statusFrame(stopped = 2)).statusView() }
        rejects("trailing bytes in a status") {
            decodeResponse(statusFrame(trailingByte = true)).also { it.statusView(); it.end() }
        }

        val taken = decodeResponse(takenFrame()).taken()
        check(taken.sessionId.size == 2)
        check(taken.outcomes.size == 1 && taken.outcomes[0].kind == OutcomeKind.Delivered)
        check(taken.outcomes[0].item == 1L && taken.outcomes[0].finished == 3L)
        check(taken.outcomes[0].reason == null)
        check(taken.sends.size == 1 && taken.sends[0].txid == "ab12" && taken.sends[0].state == SendKind.Sent)
        rejects("an outcome tag") { decodeResponse(takenFrame(outcome = 3)).taken() }
        rejects("a send tag") { decodeResponse(takenFrame(send = 8)).taken() }

        val queue = decodeResponse(queueFrame()).queue()
        check(queue.refunds.size == 1 && queue.cashOuts.size == 1 && queue.others.size == 1)
        val refund = queue.refunds[0]
        check(refund.id == 4L && refund.sale == 0L && refund.sats == 10_000L)
        check(refund.cashier == "Mia" && refund.address == "bcrt1qaddress")
        check(refund.stage.kind == StageKind.Signed && refund.stage.detail == "ab12")
        check(refund.waitingForDepth == false && refund.receivedSats == 300_000L)
        val cash = queue.cashOuts[0]
        check(cash.amount is CashAmount.All && cash.label == "till")
        check(queue.others[0].kind == EntryKind.SettingsChange)
        rejects("a cash amount tag") { decodeResponse(queueFrame(amountTag = 2)).queue() }
        rejects("a stage tag") { decodeResponse(queueFrame(stage = 6)).queue() }

        val link = decodeResponse(linkFrame()).linkState()
        check(link.reachable && link.refused == 2L)
        check(link.alerts.size == 1 && link.alerts[0].kind == AlertKind.Diverged && link.alerts[0].value == 9L)
        check(link.counters.size == 1 && link.counters[0].records == 5L)
        check(link.counters[0].checkInSeq == 3L && link.counters[0].checkInHeld == true)
        check(link.counters[0].lastCheckInAt == null)

        val old = decodeResponse(oldFrame()).oldAddresses()
        check(old.kind == OldKind.Monthly && old.count == 12L && old.daysAgo == 3L)
        check(old.next == 1_700_000_000L && !old.running && old.words?.heading == "Words")

        val held = decodeResponse(settingsFrame()).let { r -> r.option { r.settingsHeld() } }
        check(held != null && held.version == 3L && held.lightPerSale.sats == 500L)
        check(held.lightPerSale.displayUsd == 7L && held.managerOverrideCeiling.sats == 600L)
        check(held.underpaymentToleranceBps == 100L && held.minConfirmations == 1 && held.finalConfirmations == 6)
        check(held.heartbeatMaxAgeSecs == 900L && held.torOn && !held.customerWifi && held.minSaleSats == 0L)
        check(held.devices.size == 1 && held.counters.size == 1 && held.counters[0].bps == 10_000L)
        check(held.boxes.isEmpty())
        check(decodeResponse(header(0, byteArrayOf(0))).let { r -> r.option { r.settingsHeld() } } == null)
        rejects("a settings flag") { decodeResponse(header(0, byteArrayOf(2))).let { r -> r.option { r.settingsHeld() } } }
        rejects("a short settings") { decodeResponse(settingsFrame().copyOf(40)).let { r -> r.option { r.settingsHeld() } } }

        val outbox = decodeResponse(outboxFrame()).let { r -> r.list { r.outboxRow() } }
        check(outbox.size == 1 && outbox[0].kind == OutboxKind.Batch && outbox[0].id == 8L)
        check(outbox[0].txid == null && outbox[0].counter!!.size == 32 && outbox[0].version == null)
        check(outbox[0].signedAt == 1_700_000_000L && outbox[0].size == 300L && outbox[0].words.heading == "Words")
        rejects("an outbox kind tag") { decodeResponse(outboxFrame(kind = 4)).let { r -> r.list { r.outboxRow() } } }

        val pos = decodeResponse(posStateFrame()).posState()
        check(pos.setup!!.vendor == PosVendor.Clover && pos.setup!!.location == "MERCH1")
        check(pos.setup!!.tenderNames == listOf("Bitcoin") && pos.setup!!.currencies == listOf("USD"))
        check(pos.lastPollAt == 1_700_000_000L && pos.openAlerts.size == 2)
        val tender = pos.openAlerts[0]
        check(tender.key.kind == PosAlertKind.BitcoinTip && tender.raisedAt == 5L)
        check((tender.key.subject as PosSubject.Tender).tender == "T1" && tender.words.heading == "Words")
        check((pos.openAlerts[1].key.subject as PosSubject.Sale).index == 7L)
        check(pos.acknowledgedAlerts == 1L && pos.trackedOrders == 4L && pos.matches == 3L)
        check(pos.matchesByOrder == 1L && pos.retiredMatches == 9L)
        rejects("a vendor tag") { decodeResponse(posStateFrame(vendor = 2)).posState() }
        rejects("an alert kind tag") { decodeResponse(posStateFrame(kind = 9)).posState() }
        rejects("a subject tag") { decodeResponse(posStateFrame(subject = 2)).posState() }
        val reply = decodeResponse(posReplyFrame()).posReply()
        check(reply.again && !reply.moreLeft && reply.pages == 2L && reply.ordersRead == 150L)
        check(reply.newMatches == 1L && reply.newAlerts.isEmpty() && reply.cleared == 3L && reply.retired == 4L)
        val request = decodeResponse(header(0, Writer().u8(1).string("https://x/y").bytes(byteArrayOf(1, 2)).toByteArray())).posRequest()
        check(request.method == PosMethod.Post && request.url == "https://x/y" && request.body.size == 2)
        rejects("a method tag") { decodeResponse(header(0, Writer().u8(2).string("u").bytes(ByteArray(0)).toByteArray())).posRequest() }

        val pump = decodeResponse(header(0, Writer().u32(1).string("note").u8(1).toByteArray())).pump()
        check(pump.notes == listOf("note") && pump.stopped)
    }

    // ---- the requests ----

    private fun requests() {
        val frame = requestFrame(10, 1) { it.u64(1_700_000_000).u32(5) }
        check(
            hex(frame) == "4f424d6b010a000100000000000000" + "00f1536500000000" + "05000000",
        ) { hex(frame) }
        check(hex(startFrame(sampleConfig())) == START_FRAME_HEX) { hex(startFrame(sampleConfig())) }
        val reply = requestFrame(74, 1) { it.u64(5).u32(200).bytes(byteArrayOf(9)) }
        check(hex(reply) == "4f424d6b014a000100000000000000" + "0500000000000000" + "c8000000" + "0100000009") { hex(reply) }
    }

    private fun sampleConfig() =
        StartConfig(
            dir = "d",
            network = OwnerNetwork.Regtest,
            now = 1,
            ownerSecret = byteArrayOf(1),
            approverRoot = byteArrayOf(2),
            wallet = "w",
            key1Account = byteArrayOf(3),
            key3Xpub = null,
            batchMinCount = 4,
            refundDepth = 5,
            birthday = 6,
            proxy = "p",
            fixedPeers = listOf("a"),
            broadcastProxy = null,
            withoutTor = true,
            link = LinkConfig(
                route = Route.Direct("x"),
                relayKey = byteArrayOf(7),
                counters = listOf(byteArrayOf(8)),
                heartbeatEverySecs = 9,
                checkInGraceSecs = 10,
            ),
        )

    private fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    // ---- frames ----

    private fun header(status: Int, rest: ByteArray = ByteArray(0)): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'l'.code.toByte()))
        out.write(1)
        out.write(status)
        out.write(rest)
        return out.toByteArray()
    }

    private fun decodeError(frame: ByteArray): OwnerException = try {
        decodeResponse(frame)
        error("expected an OBMl error")
    } catch (e: OwnerException) {
        e
    }

    private inline fun rejects(label: String, rule: String = "response-malformed", block: () -> Unit) {
        val e = try {
            block()
            error("accepted invalid $label")
        } catch (e: OwnerException) {
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
        tone: Int = 0,
        who: Int = 4,
        heading: ByteArray = "Words".toByteArray(Charsets.UTF_8),
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
        out.write(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'l'.code.toByte()))
        out.write(1)
        out.write(status)
        string16(out, "sample-rule".toByteArray(Charsets.UTF_8))
        string16(out, "sample text".toByteArray(Charsets.UTF_8))
        words(out, tone, who, heading)
        if (trailingByte) out.write(0x55)
        return out.toByteArray()
    }

    private fun statusFrame(view: Int = 0, stopped: Int = 0, trailingByte: Boolean = false): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(view)
        out.write(0) // rescan_to absent
        words(out)
        words(out)
        out.write(Writer().u32(6).u32(7).toByteArray())
        out.write(0) // halted absent
        out.write(byteArrayOf(1, 1, 0)) // health: known, healthy, not behind
        words(out)
        out.write(Writer().u64(300_000).toByteArray())
        out.write(0) // recovery pending
        out.write(0) // recovery words absent
        out.write(stopped)
        if (trailingByte) out.write(0x55)
        return header(0, out.toByteArray())
    }

    private fun takenFrame(outcome: Int = 0, send: Int = 1): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().bytes(byteArrayOf(9, 9)).u32(1).u32(1).u8(outcome).u32(3).u8(0).toByteArray())
        words(out)
        out.write(Writer().u32(1).string("ab12").u8(send).u32(0).toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }

    private fun stage(out: ByteArrayOutputStream, kind: Int, detail: String?) {
        out.write(kind)
        if (detail == null) {
            out.write(0)
        } else {
            out.write(Writer().u8(1).string(detail).toByteArray())
        }
        words(out)
    }

    private fun queueFrame(amountTag: Int = 1, stage: Int = 3): ByteArray {
        val out = ByteArrayOutputStream()
        // One refund.
        out.write(
            Writer().u32(1).u32(4).bytes(ByteArray(32) { 1 }).u32(0).string("Mia")
                .string("bcrt1qaddress").u64(10_000).toByteArray(),
        )
        stage(out, stage, "ab12")
        out.write(Writer().u8(1).u8(0).u8(1).u64(300_000).toByteArray())
        // One cash-out of everything.
        out.write(Writer().u32(1).u32(5).string("bcrt1qother").u8(amountTag).string("till").toByteArray())
        stage(out, 0, null)
        // One settings change.
        out.write(Writer().u32(1).u32(6).u8(1).toByteArray())
        stage(out, 0, null)
        return header(0, out.toByteArray())
    }

    private fun linkFrame(): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().u8(1).u64(2).u32(1).u8(1).bytes(ByteArray(32) { 2 }).u64(9).toByteArray())
        words(out)
        out.write(Writer().u32(1).bytes(ByteArray(32) { 2 }).u64(5).bytes(ByteArray(32) { 3 }).toByteArray())
        out.write(Writer().u8(1).u64(3).u8(1).u8(1).u8(0).toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }

    private fun oldFrame(): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().u8(2).u64(12).u64(3).u8(1).u64(1_700_000_000).u8(0).u8(1).toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }

    private fun settingsFrame(): ByteArray {
        val w = Writer().u8(1).u32(3)
        for (i in 0 until 6) w.u64(if (i == 0) 500L else 1000L).u32(if (i == 0) 7 else 1)
        w.u32(100)
        for (i in 0 until 4) w.u64(if (i == 3) 600L else 1000L).u32(1)
        w.u8(1).u8(6).u32(900).bool(true).bool(false).u64(0)
        w.u32(1).bytes(ByteArray(32) { 1 })
        w.u32(1).bytes(ByteArray(32) { 2 }).u32(10_000)
        w.u32(0)
        return header(0, w.toByteArray())
    }

    private fun posStateFrame(vendor: Int = 1, kind: Int = 7, subject: Int = 0): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().u8(1).u8(vendor).string("MERCH1").u32(1).string("Bitcoin").u32(1).string("USD").toByteArray())
        out.write(Writer().u8(1).u64(1_700_000_000).u32(2).toByteArray())
        out.write(Writer().u8(kind).u8(subject).string("O1").string("T1").u64(5).toByteArray())
        words(out)
        out.write(Writer().u8(0).u8(1).bytes(ByteArray(32) { 2 }).u32(7).u64(6).toByteArray())
        words(out)
        out.write(Writer().u32(1).u32(4).u32(3).u32(1).u64(9).toByteArray())
        return header(0, out.toByteArray())
    }

    private fun posReplyFrame(): ByteArray =
        header(0, Writer().bool(true).bool(false).u32(2).u32(150).u32(1).u32(0).u32(3).u64(4).toByteArray())

    private fun outboxFrame(kind: Int = 1): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().u32(1).u32(8).u8(kind).u8(0).u8(1).bytes(ByteArray(32) { 2 }).u8(0).u8(1).u64(1_700_000_000).u32(300).toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }
}
