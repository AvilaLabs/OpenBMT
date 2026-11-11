package org.openbmt.app

/** What one scanned part did to a reading. */
sealed class UrStep {
    /** Show these words and keep scanning. */
    class Show(val words: SayWords, val received: Int = 0, val expected: Int = 0) : UrStep()

    /** The message is complete and of an accepted type. */
    class Done(val type: String, val payload: ByteArray) : UrStep()

    /** Stop scanning and show these words: nothing more can come of this scan. */
    class Fail(val words: SayWords) : UrStep()

    /** A part that changes nothing and needs no words, such as a misread. */
    object Ignore : UrStep()
}

/**
 * One reading of an animated code: one decoder of the library, fed each scanned `ur:` part. A refusal of
 * UR-12 (the parts joined to a message that failed its checks) starts the reading over and says so; the
 * other refusals of a part are misreads or another code in view and are skipped, with a word after many
 * in a row. The decoder is opened with the first part and closed by [drop]; the caller drops it when the
 * scan finishes, is cancelled or paused. [types] are the types of message the step takes; [who] is the
 * one who does the next step.
 */
class UrReading(private val call: UrCall, private val types: Set<String>, private val who: Int) {
    private var handle: Long? = null
    private var strangers = 0

    /** Whether a decoder is open. */
    val started: Boolean get() = handle != null

    /** Takes the text of one scanned part. */
    fun feed(text: String): UrStep {
        val h = handle ?: when (val opened = call.newDecoder()) {
            is UrOutcome.Ok -> opened.value.also { handle = it }
            else -> return UrStep.Fail(UrWords.readerFailed(who))
        }
        return when (val outcome = call.receive(h, text)) {
            is UrOutcome.Ok -> {
                strangers = 0
                val progress = outcome.value
                if (progress.complete) finish(h) else UrStep.Show(UrWords.reading(who, progress.received, progress.expected), progress.received, progress.expected)
            }
            is UrOutcome.Refused -> refused(outcome.code)
            is UrOutcome.Fault -> UrStep.Fail(UrWords.readerFailed(who))
        }
    }

    private fun refused(code: String): UrStep = when (code) {
        // The parts joined to a message that failed its checks: the decoder started again from nothing.
        "UR-12" -> {
            strangers = 0
            UrStep.Show(UrWords.restarted(who))
        }
        // A part of another stream: skipped, and after a run of them the person is told.
        "UR-10" -> {
            strangers++
            if (strangers % STRANGER_RUN == 0) UrStep.Show(UrWords.otherCode(who)) else UrStep.Ignore
        }
        // The call's own faults: the library is not answering as it should.
        UrCall.BAD_FRAMING, UrCall.BAD_STATE -> UrStep.Fail(UrWords.readerFailed(who))
        // A misread part (a checksum, the Bytewords, the shape) or a held-parts bound: skipped.
        else -> UrStep.Ignore
    }

    private fun finish(h: Long): UrStep = when (val outcome = call.result(h)) {
        is UrOutcome.Ok -> {
            val message = outcome.value
            if (message.type in types) UrStep.Done(message.type, message.payload) else UrStep.Fail(UrWords.wrongKind(who))
        }
        is UrOutcome.Refused ->
            if (outcome.code == UrCall.TYPE_NOT_USED) UrStep.Fail(UrWords.wrongKind(who)) else UrStep.Fail(UrWords.contentRefused(who))
        is UrOutcome.Fault -> UrStep.Fail(UrWords.readerFailed(who))
    }

    /** Closes the decoder, if one is open; what it held is gone. Safe to call more than once. */
    fun drop() {
        val h = handle ?: return
        handle = null
        call.drop(h)
    }

    companion object {
        /** Parts of another stream in a row before the person is told. */
        const val STRANGER_RUN = 15
    }
}
