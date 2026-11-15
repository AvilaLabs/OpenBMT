package org.openbmt.counter.app

import org.openbmt.counter.LnTransport

/** What one request to the shop's node came to: its answer, or why it got none (CR-114). */
sealed class LnOutcome {
    class Answer(val status: Int, val body: ByteArray) : LnOutcome()

    class Failed(val why: LnTransport) : LnOutcome()
}

/** The limits of CR-114. */
object LightningLimits {
    const val MAX_BODY_BYTES = 1_000_000
    const val CONNECT_SECS_TOR = 20
    const val CONNECT_SECS_DIRECT = 10
    const val READ_SECS = 15
}
