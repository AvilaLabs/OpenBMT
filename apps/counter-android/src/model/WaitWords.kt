package org.openbmt.counter.app

/**
 * The words of a wait that shows it is alive (CR-286): the seconds left in the ring of a short check, and how long
 * ago a payment was first seen for a wait for a block. No Android class is used.
 */
object WaitWords {
    /** CR-286: inside the ring, "5 s". */
    fun ring(secsLeft: Long): String = "$secsLeft s"

    /** CR-286: "Seen 40 s ago" under a minute, then "Seen 3 min ago" in whole minutes. */
    fun seen(secs: Long): String = if (secs < 60) "Seen $secs s ago" else "Seen ${secs / 60} min ago"

    /** Every text, for docs/screens/counter-app.md; the number is written as N. */
    fun all(): List<Pair<String, String>> = listOf(
        "ring" to ring(0).replace("0", "N"),
        "seenSeconds" to seen(0).replace("0", "N"),
        "seenMinutes" to seen(60).replace("1", "N"),
    )
}
