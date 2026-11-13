package org.openbmt.counter.app

/**
 * TP-01, TP-04, TP-05: the NFC tag's bytes for the sale on the payment screen, shared by the screen and the tag
 * service of the same process. The library made them (`Payment.tag`) with the QR code's text; the app passes them on
 * and never builds, reads or changes them. The screen sets them while a sale waits, sets them again when the request
 * changes, and clears them when the screen leaves or the sale ends. Empty is "no sale waiting".
 */
object TapTag {
    private val none = ByteArray(0)

    @Volatile
    private var bytes: ByteArray = none

    /** The tag of the sale waiting now; [tag] null or empty (a request that does not fit, TP-03) is no tag. */
    fun set(tag: ByteArray?) {
        bytes = if (tag == null || tag.isEmpty()) none else tag
    }

    /** No sale waits. */
    fun clear() {
        bytes = none
    }

    /** The bytes to answer from, or null when no sale waits (TP-04). */
    fun current(): ByteArray? = bytes.takeIf { it.isNotEmpty() }
}

/**
 * TP-20: Android is asked to send the reader's commands to the counter only while the payment screen of a sale is in
 * front: the activity is resumed and a tag is set. [apply] is called with the new answer each time it changes.
 */
class TapPreference(private val apply: (Boolean) -> Unit) {
    private var resumed = false
    private var wanted = false
    private var applied = false

    /** The activity came to the front or left it. */
    fun resumed(now: Boolean) {
        resumed = now
        update()
    }

    /** A sale's tag is set ([now] true) or cleared. */
    fun wanted(now: Boolean) {
        wanted = now
        update()
    }

    private fun update() {
        val on = resumed && wanted
        if (on != applied) {
            applied = on
            apply(on)
        }
    }
}
