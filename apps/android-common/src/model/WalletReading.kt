package org.openbmt.app

/** What one scanned part did to a wallet reading. */
sealed class WalletStep {
    /** Show these words and keep scanning; [received] of about [expected] parts are in when the library said so. */
    class Show(val words: SayWords, val received: Int = 0, val expected: Int = 0) : WalletStep()

    /** The wallet is complete and valid: its descriptor as ASCII bytes, which the caller wipes once it is handed on. */
    class Done(val text: ByteArray) : WalletStep()

    /** Stop scanning and show these words: nothing more can come of this scan. */
    class Fail(val words: SayWords) : WalletStep()

    /** A part that changes nothing and needs no words, such as a misread. */
    object Ignore : WalletStep()
}

/**
 * One reading of a wallet's animated code: one wallet reader of the library (UR-34 to UR-40), fed each scanned
 * `ur:` part of any type a wallet shows itself in (Sparrow's `crypto-output`, a crypto account, or `bytes` that
 * hold a descriptor). The library decides what a wallet is; this only carries the parts in and the text out. A
 * refusal of UR-12 starts the reading over and says so; a part of another stream or a misread is skipped, with a
 * word after many in a row; every other refusal is final for this scan. The reader is opened with the first part
 * and closed by [drop]; the caller drops it when the scan finishes, is cancelled or paused.
 */
class WalletReading(private val call: UrCall, private val who: Int) {
    private var handle: Long? = null
    private var strangers = 0

    /** Whether a reader is open. */
    val started: Boolean get() = handle != null

    /** Takes the text of one scanned part. */
    fun feed(text: String): WalletStep {
        val h = handle ?: when (val opened = call.newWalletReader()) {
            is UrOutcome.Ok -> opened.value.also { handle = it }
            else -> return WalletStep.Fail(UrWords.readerFailed(who))
        }
        return when (val outcome = call.walletReceive(h, text)) {
            is UrOutcome.Ok -> {
                strangers = 0
                val progress = outcome.value
                if (progress.complete) {
                    finish(h)
                } else {
                    WalletStep.Show(UrWords.reading(who, progress.received, progress.expected), progress.received, progress.expected)
                }
            }
            is UrOutcome.Refused -> refused(outcome.code)
            is UrOutcome.Fault -> WalletStep.Fail(UrWords.readerFailed(who))
        }
    }

    private fun refused(code: String): WalletStep = when (code) {
        "UR-12" -> {
            strangers = 0
            WalletStep.Show(UrWords.restarted(who))
        }
        "UR-10" -> {
            strangers++
            if (strangers % STRANGER_RUN == 0) WalletStep.Show(UrWords.otherCode(who)) else WalletStep.Ignore
        }
        UrCall.BAD_FRAMING, UrCall.BAD_STATE -> WalletStep.Fail(UrWords.readerFailed(who))
        // A code that is not of a wallet's type.
        WRONG_KIND -> WalletStep.Fail(UrWords.wrongKind(who))
        // The wallet reader's own refusals: what the code holds is not a wallet this app takes.
        in WALLET_REFUSALS -> WalletStep.Fail(UrWords.contentRefused(who))
        // A misread part (a checksum, the Bytewords, the shape): skipped.
        else -> WalletStep.Ignore
    }

    private fun finish(h: Long): WalletStep = when (val outcome = call.walletResult(h)) {
        is UrOutcome.Ok -> WalletStep.Done(outcome.value)
        is UrOutcome.Refused -> if (outcome.code == WRONG_KIND) WalletStep.Fail(UrWords.wrongKind(who)) else WalletStep.Fail(UrWords.contentRefused(who))
        is UrOutcome.Fault -> WalletStep.Fail(UrWords.readerFailed(who))
    }

    /** Closes the reader, if one is open; what it held is gone. Safe to call more than once. */
    fun drop() {
        val h = handle ?: return
        handle = null
        call.walletDrop(h)
    }

    companion object {
        /** Parts of another stream in a row before the person is told. */
        const val STRANGER_RUN = 15

        /** UR-34: the code is not of a type a wallet shows itself in. */
        const val WRONG_KIND = "UR-34"

        /** UR-35 to UR-40: malformed, unsupported, a test network, a secret key, too large, not enough. */
        private val WALLET_REFUSALS = setOf("UR-35", "UR-36", "UR-37", "UR-38", "UR-39", "UR-40")
    }
}
