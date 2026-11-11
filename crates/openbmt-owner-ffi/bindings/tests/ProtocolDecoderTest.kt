package org.openbmt.owner

import java.io.ByteArrayOutputStream

/** Plain JVM regression harness for the production OBMl decoder and the OBMk encoder; run its main. */
object ProtocolDecoderTest {
    /**
     * The Key app's answers, made by the Key app's own library in a Rust test of this crate
     * (`oc_41_the_answers_of_the_kotlin_harness_are_the_key_apps` fails when these differ): one that holds
     * key 2, one that holds none, and the text of a key 2 that comes from elsewhere.
     */
    private const val ANSWER_WITH_KEY2 =
        "4f424d76010301160cf12f043587cf042b03e06d800000020b9df360664015eff42762c02d0e229979e83836f690bf26044b35cd2162c64e03d8a089fd6db0e3d1f85decbdf92b091ea42271f406fa459b52ff7889c1ea219056c4fac3043587cf04686ae4bf80000002d0b08dc903742e8e907aed44ced618ec55545adf33a8f3cfe444470bb19fe79b0241ea5cb48ecc741308624baf221aa4e3057447056e4fa48749572714abff29f1134a148c41aca288696437f28ac9a60f9f965ac2cc350c42fc447249b539b981"
    private const val ANSWER_WITHOUT_KEY2 =
        "4f424d7601010056c4fac3043587cf04686ae4bf80000002d0b08dc903742e8e907aed44ced618ec55545adf33a8f3cfe444470bb19fe79b0241ea5cb48ecc741308624baf221aa4e3057447056e4fa48749572714abff29f1134a148c41aca288696437f28ac9a60f9f965ac2cc350c42fc447249b539b981"
    private const val KEY2_TEXT =
        "[8dfc9b34/48h/1h/0h/2h]tpubDEXiq2SVhhqALktxfVFgj3C9M3T2G7xL11iezYg2LJAf245YkNyqp2K9TrvHABDCp2232k34UegU4aKEtUZNigit8EEqoLNe2JKMzMiLwYq"

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
        ceremonyFrames()
        ceremonyThroughTheLibrary()
        setupLockThroughTheLibrary()
        priceThroughTheLibrary()
        println("ProtocolDecoderTest: ok")
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
        check(link.batches.size == 1 && link.batches[0].batches == 2L && link.batches[0].since == 77L)
        check(link.batches[0].words.heading == "Words")

        val old = decodeResponse(oldFrame()).oldAddresses()
        check(old.kind == OldKind.Monthly && old.count == 12L && old.daysAgo == 3L)
        check(old.next == 1_700_000_000L && !old.running && old.words?.heading == "Words")

        val held = decodeResponse(settingsFrame()).let { r -> r.option { r.settingsHeld() } }
        check(held != null && held.version == 3L && held.lightPerSale.sats == 500L)
        check(held.lightPerSale.displayUsd == 7L && held.managerOverrideCeiling.sats == 600L)
        check(held.underpaymentToleranceBps == 100L && held.minConfirmations == 1 && held.finalConfirmations == 6)
        check(held.heartbeatMaxAgeSecs == 900L && held.torOn && !held.customerWifi && held.minSaleSats == 0L)
        check(held.devices.size == 1 && held.counters.size == 1 && held.counters[0].bps == 10_000L)
        check(held.tipsOn && held.tipPercents == listOf(10, 15, 20))
        check(held.boxes.isEmpty())
        check(held.lightningNode!!.size == 33 && held.lightningNode!![0].toInt() == 2)
        check(held.currency == ShopCurrency.Jpy && held.currency.code == "JPY" && held.currency.plural == "yen")
        check(decodeResponse(header(0, byteArrayOf(0))).let { r -> r.option { r.settingsHeld() } } == null)
        rejects("a settings flag") { decodeResponse(header(0, byteArrayOf(2))).let { r -> r.option { r.settingsHeld() } } }
        rejects("a short settings") { decodeResponse(settingsFrame().copyOf(40)).let { r -> r.option { r.settingsHeld() } } }

        val preview = decodeResponse(previewFrame()).settingsPreview()
        check(preview.version == 4L && preview.changes.size == 2 && preview.needsKeyApp && !preview.nothingChanged)
        check(preview.changes[0].field == "refund_cap" && preview.changes[0].counter == null)
        check(preview.changes[0].old == "$100" && preview.changes[0].new == "$150" && preview.changes[0].loosening)
        check(preview.changes[1].counter!!.size == 32 && preview.changes[1].new == "33.33%")
        check(!preview.changes[1].loosening && preview.sharesTotalBps == 8_333L && preview.usdPerBtc == 98_500L && preview.satsPerUsd == 1_015L)
        check(preview.settings.size == 2)
        check(preview.currency == ShopCurrency.Eur && preview.currency.label == "Euro (EUR)")
        rejects("a currency tag that is not a choice") { decodeResponse(settingsFrame().also { it[it.size - 1] = 0 }).let { r -> r.option { r.settingsHeld() } } }
        rejects("a currency tag past the list") { decodeResponse(settingsFrame().also { it[it.size - 1] = 11 }).let { r -> r.option { r.settingsHeld() } } }
        rejects("a preview flag") { decodeResponse(previewFrame(needsKeyApp = 2)).settingsPreview() }
        rejects("a short preview") { decodeResponse(previewFrame().copyOf(30)).settingsPreview() }

        val scan = decodeResponse(counterScanFrame()).counterScan()
        check(scan.pairing == 4L && scan.counter.size == 32 && scan.name == "Front counter")
        check(scan.sessionWords == listOf("one", "two", "three", "four") && scan.needsRate)
        check(scan.windowSecs == 600L && scan.shares.size == 2 && scan.shares[1].bps == 5_000L)
        check(scan.words.heading == "Words")
        rejects("a short counter scan") { decodeResponse(counterScanFrame().copyOf(30)).counterScan() }
        val queuedCounter = decodeResponse(counterQueuedFrame()).counterQueued()
        check(queuedCounter.settings == 7L && queuedCounter.version == 2L)
        check(queuedCounter.batch == 8L && queuedCounter.batchCount == 1_000L)
        check(queuedCounter.shares.size == 1 && queuedCounter.shares[0].bps == 10_000L)
        val waitingCounters = decodeResponse(waitingFrame()).let { r -> r.list { r.waitingCounter() } }
        check(waitingCounters.size == 1 && waitingCounters[0].name == "Front counter")
        check(waitingCounters[0].counter.size == 32)
        val bundle = decodeResponse(bundleFrame()).counterBundle()
        check(bundle.code.size == 3 && bundle.withBatch && bundle.words.heading == "Words")
        rejects("a bundle flag") { decodeResponse(bundleFrame(flag = 2)).counterBundle() }
        // CR-132: the relay for counters is the frame's bytes, nothing else.
        decodeResponse(header(0, Writer().bytes(byteArrayOf(9, 8)).toByteArray())).let { r ->
            check(r.bytes().contentEquals(byteArrayOf(9, 8)))
            r.end()
        }

        val boxFound = decodeResponse(header(0, Writer().bytes(byteArrayOf(1, 2, 3)).bytes(ByteArray(32) { 4 }).bool(true).toByteArray())).boxFound()
        check(boxFound.claim.size == 3 && boxFound.boxKey.size == 32 && boxFound.needsRate)
        rejects("a short Box claim") { decodeResponse(header(0, Writer().bytes(byteArrayOf(1)).toByteArray())).boxFound() }
        rejects("a Box rate flag") { decodeResponse(header(0, Writer().bytes(byteArrayOf(1)).bytes(ByteArray(32)).u8(2).toByteArray())).boxFound() }
        val boxQueued = decodeResponse(
            header(0, Writer().u32(1).u32(2).u32(3).bytes(ByteArray(32) { 5 }).u32(4).string("a").string("b").string("c").string("d").toByteArray()),
        ).boxQueued()
        check(boxQueued.claimId == 1L && boxQueued.settingsId == 2L && boxQueued.settingsVersion == 3L)
        check(boxQueued.boxKey.size == 32 && boxQueued.words == listOf("a", "b", "c", "d"))
        val waitingBoxes = decodeResponse(header(0, Writer().u32(1).bytes(ByteArray(32) { 6 }).toByteArray())).let { r -> r.list { r.waitingBox() } }
        check(waitingBoxes.size == 1 && waitingBoxes[0].boxKey.size == 32)
        val boxDelivered = decodeResponse(
            header(0, Writer().bytes(ByteArray(32) { 7 }).u32(2).bytes(ByteArray(32) { 8 }).bytes(ByteArray(32) { 9 }).u32(5).toByteArray()),
        ).boxDelivered()
        check(boxDelivered.boxKey.size == 32 && boxDelivered.counters.size == 2 && boxDelivered.settingsVersion == 5L)
        val pendingBoxes = decodeResponse(
            header(0, Writer().u32(1).bytes(ByteArray(32) { 6 }).u32(2).string("a").string("b").u8(1).toByteArray()),
        ).let { r -> r.list { r.pendingBox() } }
        check(pendingBoxes.size == 1 && pendingBoxes[0].boxKey.size == 32 && pendingBoxes[0].words == listOf("a", "b") && pendingBoxes[0].welcomeReady)
        rejects("a pending Box flag") { decodeResponse(header(0, Writer().u32(1).bytes(ByteArray(32)).u32(0).u8(2).toByteArray())).let { r -> r.list { r.pendingBox() } } }
        rejects("a short Box delivery") { decodeResponse(header(0, Writer().bytes(ByteArray(32)).u32(2).bytes(ByteArray(32)).toByteArray())).boxDelivered() }

        val outbox = decodeResponse(outboxFrame()).let { r -> r.list { r.outboxRow() } }
        check(outbox.size == 1 && outbox[0].kind == OutboxKind.Batch && outbox[0].id == 8L)
        check(outbox[0].txid == null && outbox[0].counter!!.size == 32 && outbox[0].version == null)
        check(outbox[0].signedAt == 1_700_000_000L && outbox[0].size == 300L && outbox[0].words.heading == "Words")
        rejects("an outbox kind tag") { decodeResponse(outboxFrame(kind = 5)).let { r -> r.list { r.outboxRow() } } }

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


    // ---- the key ceremony: frames, then the real library ----

    private fun ceremonyFrames() {
        // The request of make key 1, as the Rust test writes it: network Regtest (3), no words.
        check(hex(requestFrame(80, 0) { it.u8(3).u8(0) }) == "4f424d6b015000" + "0000000000000000" + "0300") {
            hex(requestFrame(80, 0) { it.u8(3).u8(0) })
        }
        val made = decodeResponse(key1MadeFrame()).key1Made()
        check(made.record.size == 2 && made.key1.fingerprint == "aabbccdd" && made.key1.originPath == "48h/1h/0h/2h")
        check(made.ownerRoot.size == 32 && made.ownerRootWords == listOf("one", "two", "three", "four"))
        check(made.words == null && made.stateWords.heading == "Words")
        check(decodeResponse(key1MadeFrame(withWords = true)).key1Made().words == listOf("a", "b"))
        rejects("an option flag of the key 1 words") { decodeResponse(key1MadeFrame(wordsFlag = 2)).key1Made() }
        val answer = decodeResponse(answerFrame()).answerView()
        check(answer.network == OwnerNetwork.Signet && answer.key2 == null && answer.key3.fingerprint == "11223344")
        check(answer.keyAppRoot.size == 32 && answer.keyAppRootWords.size == 4)
        check(decodeResponse(answerFrame(withKey2 = true)).answerView().key2?.fingerprint == "aabbccdd")
        rejects("a network tag") { decodeResponse(answerFrame(network = 4)).answerView() }
        rejects("a short answer") { decodeResponse(answerFrame().copyOf(20)).answerView() }
        val stage = decodeResponse(header(0, Writer().u8(2).toByteArray() + wordsBytes())).setupStageView()
        check(stage.stage == 2 && stage.words.heading == "Words")
        val defaults = decodeResponse(header(0, Writer().u32(1_000).u32(6).u32(967_680).toByteArray())).ceremonyDefaults()
        check(defaults.batchMinCount == 1_000L && defaults.refundDepth == 6L && defaults.birthday == 967_680L)
        rejects("trailing bytes in the defaults") {
            decodeResponse(header(0, Writer().u32(1).u32(2).u32(3).u8(0).toByteArray())).also { it.ceremonyDefaults(); it.end() }
        }
    }

    private fun wordsBytes(): ByteArray {
        val out = ByteArrayOutputStream()
        words(out)
        return out.toByteArray()
    }

    private fun partBytes(fingerprint: String) =
        Writer().string(fingerprint).string("48h/1h/0h/2h").string("xpubplaceholder").toByteArray()

    private fun key1MadeFrame(withWords: Boolean = false, wordsFlag: Int = if (withWords) 1 else 0): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().bytes(byteArrayOf(1, 2)).toByteArray())
        out.write(partBytes("aabbccdd"))
        out.write(Writer().bytes(ByteArray(32) { 5 }).u32(4).string("one").string("two").string("three").string("four").u8(wordsFlag).toByteArray())
        if (wordsFlag == 1) out.write(Writer().u32(2).string("a").string("b").toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }

    private fun answerFrame(network: Int = 2, withKey2: Boolean = false): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().u8(network).toByteArray())
        if (withKey2) {
            out.write(Writer().u8(1).toByteArray())
            out.write(partBytes("aabbccdd"))
        } else {
            out.write(Writer().u8(0).toByteArray())
        }
        out.write(partBytes("11223344"))
        out.write(Writer().bytes(ByteArray(32) { 6 }).u32(4).string("w").string("x").string("y").string("z").toByteArray())
        return header(0, out.toByteArray())
    }

    private fun unhex(text: String): ByteArray = ByteArray(text.length / 2) { text.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

    /** Runs [block], which must be refused by the library with a rule that starts with [prefix] and has words. */
    private inline fun refusedBy(label: String, prefix: String, block: () -> Unit): OwnerException {
        val e = try {
            block()
            error("accepted: $label")
        } catch (e: OwnerException) {
            e
        }
        check(e.status == 1 && e.rule.startsWith(prefix)) { "$label: status ${e.status}, rule ${e.rule}" }
        check(e.words != null && e.words!!.heading.isNotEmpty() && e.words!!.next.isNotEmpty()) { "$label: no words" }
        return e
    }

    /**
     * The price of a bitcoin through the real library (OA-101): the table of sources, the price two replies
     * agree on, and the library's refusals (sources that disagree, too few answers) with their words.
     */
    private fun priceThroughTheLibrary() {
        val sources = OwnerPrice.sources()
        check(sources.map { it.id } == listOf("kraken", "coinbase", "bitstamp", "gemini")) { "the table: ${sources.map { it.id }}" }
        check(sources.all { it.url.startsWith("https://") })
        fun last(source: String, price: String) = PriceReply(source, "{\"last\":\"$price\"}".toByteArray(), 1_700_000_000L)
        val agreed = OwnerPrice.agree(listOf(last("bitstamp", "98500.50"), last("gemini", "98500.50")), 1_700_000_000L)
        check(agreed.usdPerBtc == 98_501L && agreed.minorPerBtc == 9_850_050L && agreed.sourceLabel == "med.bg") { "agreed: ${agreed.usdPerBtc} ${agreed.sourceLabel}" }
        refusedBy("sources that disagree", "RT-33") { OwnerPrice.agree(listOf(last("bitstamp", "98000"), last("gemini", "110000")), 1_700_000_000L) }
        refusedBy("one source", "RT-31") { OwnerPrice.agree(listOf(last("bitstamp", "98000")), 1_700_000_000L) }
        refusedBy("no source", "RT-31") { OwnerPrice.agree(emptyList(), 1_700_000_000L) }
        refusedBy("a reply that is not a price", "RT-31") {
            OwnerPrice.agree(listOf(PriceReply("bitstamp", "nope".toByteArray(), 1_700_000_000L), last("gemini", "98000")), 1_700_000_000L)
        }
        refusedBy("a source that is not in the table", "RT-31") { OwnerPrice.agree(listOf(PriceReply("nowhere", ByteArray(0), 1L)), 1L) }
    }

    /**
     * The lock on the setup text through the real library, at the real cost (Argon2id, 64 MiB, 3 passes): a text
     * is locked, told from a bare one, opened, and its PIN changed, and each refusal has its rule and its words.
     * The time of one lock or unlock is printed.
     */
    private fun setupLockThroughTheLibrary() {
        val text = "owner_secret=00112233\nkey1_account=44556677\n".toByteArray()
        check(!OwnerLock.isLocked(text) && !OwnerLock.isLocked(ByteArray(0)))
        val started = System.nanoTime()
        val locked = OwnerLock.lock("482913", text)
        val lockMs = (System.nanoTime() - started) / 1_000_000
        check(OwnerLock.isLocked(locked) && String(locked, Charsets.ISO_8859_1).startsWith("OBMx"))
        check(!String(locked, Charsets.ISO_8859_1).contains("owner_secret"))
        val opened = System.nanoTime()
        check(OwnerLock.unlock(locked, "482913").contentEquals(text))
        val unlockMs = (System.nanoTime() - opened) / 1_000_000
        println("ProtocolDecoderTest: lock $lockMs ms, unlock $unlockMs ms")
        check(!OwnerLock.lock("482913", text).contentEquals(locked)) { "a new salt and a new nonce each time" }
        val moved = OwnerLock.changePin(locked, "482913", "a longer passphrase")
        check(OwnerLock.unlock(moved, "a longer passphrase").contentEquals(text))
        refusedBy("the old PIN after a change", "pin-wrong") { OwnerLock.unlock(moved, "482913") }
        check(OwnerLock.unlock(locked, "482913").contentEquals(text)) { "the old frame is as it was" }
        refusedBy("a wrong PIN", "pin-wrong") { OwnerLock.unlock(locked, "482914") }
        refusedBy("a wrong current PIN", "pin-wrong") { OwnerLock.changePin(locked, "482914", "654321") }
        refusedBy("a weak PIN", "pin-weak") { OwnerLock.lock("12345", text) }
        refusedBy("a weak new PIN", "pin-weak") { OwnerLock.changePin(locked, "482913", "123") }
        refusedBy("a PIN that is too long", "pin-long") { OwnerLock.lock("9".repeat(129), text) }
        refusedBy("a cut frame", "OBMx") { OwnerLock.unlock(locked.copyOf(locked.size - 1), "482913") }
        refusedBy("a bare text", "OBMx") { OwnerLock.unlock(text, "482913") }
        val flipped = locked.copyOf().also { it[it.size - 1] = (it[it.size - 1].toInt() xor 1).toByte() }
        refusedBy("a changed ciphertext", "pin-wrong") { OwnerLock.unlock(flipped, "482913") }
    }

    /**
     * The whole ceremony through the real library, loaded from java.library.path: the defaults, key 1, the Key
     * app's answer, the wallet, the stage at each step, the finish, the open, and each kind of refusal. The
     * sealed records are the library's; this harness only hands them back.
     */
    private fun ceremonyThroughTheLibrary() {
        val regtest = OwnerCeremony.defaults(OwnerNetwork.Regtest)
        check(regtest.batchMinCount == 1_000L && regtest.refundDepth == 6L && regtest.birthday == 0L)
        val mainnet = OwnerCeremony.defaults(OwnerNetwork.Mainnet)
        check(mainnet.batchMinCount == regtest.batchMinCount && mainnet.birthday == 967_680L)
        check(OwnerCeremony.defaults(OwnerNetwork.Signet).birthday == 0L)

        // Step 1: key 1, with no words to show.
        val made = OwnerCeremony.makeKey1(Key1Args(OwnerNetwork.Regtest, showWords = false))
        check(made.words == null && made.record.isNotEmpty() && made.ownerRoot.size == 32)
        check(made.ownerRootWords.size == 4 && made.key1.fingerprint.length == 8 && made.stateWords.heading.isNotEmpty())
        val first = OwnerCeremony.stage(made.record)
        check(first.stage == 1 && first.words.heading.isNotEmpty())
        // The words only when asked for, 24 of them.
        val shown = OwnerCeremony.makeKey1(Key1Args(OwnerNetwork.Regtest, showWords = true))
        check(shown.words?.size == 24)

        // Step 2: the Key app's answer.
        val answer = unhex(ANSWER_WITH_KEY2)
        val read = OwnerCeremony.readAnswer(answer)
        check(read.key2 != null && read.network == OwnerNetwork.Regtest && read.keyAppRoot.size == 32)
        check(read.keyAppRootWords.size == 4 && read.keyAppRootWords != made.ownerRootWords)
        val bare = OwnerCeremony.readAnswer(unhex(ANSWER_WITHOUT_KEY2))
        check(bare.key2 == null && bare.network == OwnerNetwork.Testnet)
        refusedBy("a cut answer", "SR-") { OwnerCeremony.readAnswer(answer.copyOf(answer.size - 3)) }
        refusedBy("a code that is not an answer", "SR-") { OwnerCeremony.readAnswer(byteArrayOf(1, 2, 3)) }

        // Step 3: the wallet. Key 2 comes in the answer or as text, never both and never neither.
        val minCount = regtest.batchMinCount
        refusedBy("no key 2 at all", "OC-41") {
            OwnerCeremony.buildWallet(BuildArgs(made.record, unhex(ANSWER_WITHOUT_KEY2), null, minCount))
        }
        refusedBy("key 2 twice", "OC-41") {
            OwnerCeremony.buildWallet(BuildArgs(made.record, answer, KEY2_TEXT, minCount))
        }
        refusedBy("a key 2 text that is not a key", "SR-") {
            OwnerCeremony.buildWallet(BuildArgs(made.record, unhex(ANSWER_WITHOUT_KEY2), "not a key", minCount))
        }
        refusedBy("a record that is cut", "OC-44") {
            OwnerCeremony.buildWallet(BuildArgs(made.record.copyOf(made.record.size - 2), answer, null, minCount))
        }
        val mainnetKey = OwnerCeremony.makeKey1(Key1Args(OwnerNetwork.Mainnet, showWords = false))
        refusedBy("an answer of another network", "") {
            OwnerCeremony.buildWallet(BuildArgs(mainnetKey.record, answer, null, minCount))
        }
        val viaText = OwnerCeremony.buildWallet(BuildArgs(made.record, unhex(ANSWER_WITHOUT_KEY2), KEY2_TEXT, minCount))
        check(OwnerCeremony.stage(viaText.record).stage == 2 && viaText.walletSetup.isNotEmpty())

        val built = OwnerCeremony.buildWallet(BuildArgs(made.record, answer, null, minCount))
        check(OwnerCeremony.stage(built.record).stage == 2)
        check(built.walletSetup.size > 4 && built.walletSetup.copyOf(4).contentEquals("OBMw".toByteArray(Charsets.US_ASCII)))
        check(built.checksumWords.size == 4 && built.fingerprints.size == 3 && built.fingerprints.all { it.length == 8 })
        check(built.fingerprints[0] == made.key1.fingerprint && built.fingerprints[1] == read.key2!!.fingerprint)
        check(built.fingerprints[2] == read.key3.fingerprint && built.description.startsWith("wsh("))
        check(built.firstReceiveAddress.startsWith("bcrt1") && built.pairs.size == 3)
        check(built.pairs.all { it.holderA in 1..4 && it.holderB in 1..4 && it.keyA in 1..3 && it.keyB in 1..3 })
        check(built.pairsWords.heading.isNotEmpty() && built.stateWords.heading.isNotEmpty())
        // The same inputs make the same wallet, so the Key app shows the same words.
        val again = OwnerCeremony.buildWallet(BuildArgs(made.record, answer, null, minCount))
        check(again.checksumWords == built.checksumWords && again.walletSetup.contentEquals(built.walletSetup))

        // Step 4: the owner's word. Nothing opens before it.
        refusedBy("open before the owner confirms", "OC-45") { OwnerCeremony.open(built.record) }
        refusedBy("finish at stage 1", "OC-45") { OwnerCeremony.finish(FinishArgs(made.record, 6, 0)) }
        refusedBy("a refund depth of 0", "OC-45") { OwnerCeremony.finish(FinishArgs(built.record, 0, 0)) }
        refusedBy("a refund depth of 101", "OC-45") { OwnerCeremony.finish(FinishArgs(built.record, 101, 0)) }
        refusedBy("a stage of garbage", "OC-44") { OwnerCeremony.stage(byteArrayOf(1, 2, 3)) }
        val done = OwnerCeremony.finish(FinishArgs(built.record, regtest.refundDepth, regtest.birthday))
        check(OwnerCeremony.stage(done.record).stage == 3 && done.stateWords.heading.isNotEmpty())
        refusedBy("finish twice", "OC-45") { OwnerCeremony.finish(FinishArgs(done.record, 6, 0)) }

        // The values the node starts from.
        val fields = OwnerCeremony.open(done.record)
        check(fields.network == OwnerNetwork.Regtest && fields.wallet == built.description)
        check(fields.ownerSecret.size == 32 && fields.approverRoot.contentEquals(read.keyAppRoot))
        check(fields.key1Account.size == 78 && fields.key3Xpub == read.key3.accountXpub)
        check(fields.batchMinCount == minCount && fields.refundDepth == 6L && fields.birthday == 0L)
    }

    // ---- the requests ----

    private fun requests() {
        val frame = requestFrame(10, 1) { it.u64(1_700_000_000).u32(5) }
        check(
            hex(frame) == "4f424d6b010a000100000000000000" + "00f1536500000000" + "05000000",
        ) { hex(frame) }
        check(hex(startFrame(sampleConfig())) == START_FRAME_HEX) { hex(startFrame(sampleConfig())) }
        val edit = requestFrame(47, 1) { it.settingsEdit(sampleEdit()) }
        check(hex(edit) == EDIT_FRAME_HEX) { hex(edit) }
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
        out.write(Writer().u32(1).bytes(ByteArray(32) { 2 }).u32(2).u64(77).toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }

    private fun counterScanFrame(): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().u32(4).bytes(ByteArray(32) { 3 }).string("Front counter").toByteArray())
        out.write(Writer().u32(4).string("one").string("two").string("three").string("four").toByteArray())
        out.write(Writer().bool(true).u32(600).u32(2).bytes(ByteArray(32) { 3 }).u32(5_000).toByteArray())
        out.write(Writer().bytes(ByteArray(32) { 4 }).u32(5_000).toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }

    private fun counterQueuedFrame(): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().u32(7).u32(2).u32(8).u32(1_000).u32(1).bytes(ByteArray(32) { 3 }).u32(10_000).toByteArray())
        words(out)
        return header(0, out.toByteArray())
    }

    private fun waitingFrame(): ByteArray =
        header(0, Writer().u32(1).bytes(ByteArray(32) { 3 }).string("Front counter").toByteArray())

    private fun bundleFrame(flag: Int = 1): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(Writer().bytes(byteArrayOf(1, 2, 3)).u8(flag).toByteArray())
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
        w.bool(true).bytes(byteArrayOf(10, 15, 20))
        w.u32(1).bytes(ByteArray(32) { 1 })
        w.u32(1).bytes(ByteArray(32) { 2 }).u32(10_000)
        w.u32(0)
        w.u8(1).bytes(ByteArray(33) { if (it == 0) 2 else 7 })
        w.u8(4)
        return header(0, w.toByteArray())
    }

    /** A preview of two changes, the second a counter's share, as `settings_preview` answers. */
    private fun previewFrame(needsKeyApp: Int = 1): ByteArray {
        val w = Writer().u32(4).u32(2)
        w.string("refund_cap").string("Largest single refund").u8(0).string("$100").string("$150").bool(true)
        w.string("counters").string("Counter's share").u8(1).bytes(ByteArray(32) { 5 })
            .string("50%").string("33.33%").bool(false)
        w.u8(needsKeyApp).bool(false).u32(8_333).u8(1).u64(98_500).u8(1).u64(1_015).bytes(byteArrayOf(9, 9)).u8(2)
        return header(0, w.toByteArray())
    }

    /** The request `previewSettings` writes for [sampleEdit]; the Rust test of the same name writes it too. */
    private const val EDIT_FRAME_HEX =
        "4f424d6b012f00010000000000000001e803000000000000010a0000000000013200000000000001640000000000000103000001010000000000000000010000000000"

    private fun sampleEdit() =
        SettingsEditArgs(
            usdPerBtc = 1_000,
            lightPerSale = 10,
            nodeBudget = 50,
            refundCap = 100,
            minConfirmations = 3,
            torOn = true,
            lightning = LightningAction.Remove,
        )

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
