package org.openbmt.app

/** What one scanned part did to a BBQr reading. */
sealed class BbqrStep {
    /** [have] of [total] parts are read; keep scanning. */
    class Show(val have: Int, val total: Int) : BbqrStep()

    /** Show these words and keep scanning: many parts in a row were not part of this set. */
    class Say(val words: SayWords) : BbqrStep()

    /** Every part is read and the file is of an accepted kind. */
    class Done(val kind: Char, val payload: ByteArray) : BbqrStep()

    /** Stop scanning and show these words: nothing more can come of this scan. */
    class Fail(val words: SayWords) : BbqrStep()

    /** A part that changes nothing and needs no words, such as a misread. */
    object Ignore : BbqrStep()
}

/**
 * One reading of a BBQr set: one joiner of the library, fed each scanned part in any order and with
 * repeats (BQ-10). A part the joiner refuses is a misread or a part of another set and is skipped, with a
 * word after many in a row. The joiner is opened with the first part and closed by [drop]; the caller drops
 * it when the scan finishes, is cancelled or paused. [kinds] are the kinds of file the step takes; [who] is
 * the one who does the next step.
 */
class BbqrReading(private val call: UrCall, private val kinds: Set<Char>, private val who: Int) {
    private var handle: Long? = null
    private var strangers = 0

    /** Whether a joiner is open. */
    val started: Boolean get() = handle != null

    /** Takes the text of one scanned part. */
    fun feed(text: String): BbqrStep {
        val h = handle ?: when (val opened = call.newJoiner()) {
            is UrOutcome.Ok -> opened.value.also { handle = it }
            else -> return BbqrStep.Fail(UrWords.readerFailed(who))
        }
        return when (val outcome = call.joinerAdd(h, text)) {
            is UrOutcome.Ok -> {
                strangers = 0
                val progress = outcome.value
                if (progress.complete) finish(h) else BbqrStep.Show(progress.have, progress.total)
            }
            is UrOutcome.Refused -> {
                strangers++
                if (strangers % STRANGER_RUN == 0) BbqrStep.Say(UrWords.otherCode(who)) else BbqrStep.Ignore
            }
            is UrOutcome.Fault -> BbqrStep.Fail(UrWords.readerFailed(who))
        }
    }

    private fun finish(h: Long): BbqrStep = when (val outcome = call.joinerResult(h)) {
        is UrOutcome.Ok -> {
            val message = outcome.value
            if (message.kind in kinds) BbqrStep.Done(message.kind, message.payload) else BbqrStep.Fail(UrWords.wrongKind(who))
        }
        is UrOutcome.Refused -> BbqrStep.Fail(UrWords.contentRefused(who))
        is UrOutcome.Fault -> BbqrStep.Fail(UrWords.readerFailed(who))
    }

    /** Closes the joiner, if one is open; what it held is gone. Safe to call more than once. */
    fun drop() {
        val h = handle ?: return
        handle = null
        call.joinerDrop(h)
    }

    companion object {
        /** Parts the joiner refused in a row before the person is told. */
        const val STRANGER_RUN = 15
    }
}
