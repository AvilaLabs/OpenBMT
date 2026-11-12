package org.openbmt.counter.app

/** CR-213 to CR-216: the labels of the request to send and of the note on a sale. */
object WordsOfRequests {
    /** CR-213: the quieter button under the keypad's main button, and the title of the page it opens. */
    const val REQUEST = "Request to send"

    /** CR-213: the button that makes the request. */
    const val MAKE = "Make the request"

    /** CR-214: the button that gives the payment link to the share sheet. */
    const val SHARE_LINK = "Share the link"

    /** CR-214: the button that gives the code, as an image, to the share sheet. */
    const val SHARE_CODE = "Share the code"

    /** CR-215: the heading of the list of open requests on Home. */
    const val OPEN_REQUESTS = "Open requests"

    /** CR-216: the label of a sale's note. */
    const val NOTE = "Note"

    /** CR-216: the button that adds a note to a sale that has none. */
    const val ADD_NOTE = "Add a note"

    /** CR-213: the hint of the note field. */
    const val NOTE_HINT = "Optional. Like a booking name or date."

    /** CR-213: the lifetimes a request can have, the label of each chip and its days. */
    val LIFETIMES = listOf("1 day" to 1, "3 days" to 3, "7 days" to 7)
}
