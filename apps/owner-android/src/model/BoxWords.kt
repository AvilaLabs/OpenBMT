package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The words of the app's own states while a Box is connected (OA-40 to OA-48,
 * docs/SCREEN-RULES.md). The refusals of the library (a Box that cannot be
 * reached, that is not a Box, that has no invitation open, that is already
 * paired, that refused its welcome) are the library's words and are shown as
 * they come. These are what the app decides itself. They are part of
 * `AppWords.all()` and of docs/screens/owner-app.md.
 */
object BoxWords {
    private fun w(tone: Int, heading: String, what: String, who: Int, next: String) =
        SayWords(tone, who, heading, what, next, null, null, null, null)

    /** What was typed is not the shape of a Box's address. */
    val addressNotRead = w(
        Tones.STOP, "Address not read",
        "The Box's address is host:port on your network, or a v3 .onion name and a port, so nothing was tried.",
        Who.OWNER,
        "Type it again, for example 192.168.1.20:8080.",
    )

    /** The Box is being asked for its claim. */
    val looking = w(
        Tones.INFO, "Looking for the Box",
        "The phone is asking the Box for its claim, which can take up to a minute.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** A phone with no settings needs today's price of a bitcoin to start them. */
    val priceNeeded = w(
        Tones.INFO, "Price for the Box",
        "This phone holds no settings yet, and the starting limits are in dollars, so it needs today's price of a bitcoin.",
        Who.OWNER,
        "Type today's price of one bitcoin in whole dollars, then queue the Box.",
    )

    /** What the owner does with the four words. */
    val compare = w(
        Tones.INFO, "Compare the four words",
        "The Box shows four words on its own screen. They must be the same as these, and the Key app will show them too.",
        Who.OWNER,
        "If they differ, go back. Nothing is paired until the Key app approves.",
    )

    /** The Box's claim and the settings that add it are queued. */
    val queued = w(
        Tones.INFO, "Box queued",
        "The Box's claim and the settings that add it wait for the Key app.",
        Who.OWNER,
        "Build the request for the Key app, approve there, and bring its response back.",
    )

    /** The welcome is being handed to the Box. */
    val handing = w(
        Tones.INFO, "Handing over the welcome",
        "The Box is being given its welcome and the settings, which can take up to a minute.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** The welcome did not reach the Box after the Key app approved it: what is kept, and what to do. */
    val handoverKept = w(
        Tones.INFO, "The approval is kept",
        "The Key app's approval stays queued on this phone. Nothing was lost and the Box is not paired yet.",
        Who.OWNER,
        "Try again here once the Box answers. If the invitation ran out, make a new one on the Key app.",
    )

    /** The Box took its welcome and the link to it was saved. */
    val paired = w(
        Tones.GOOD, "Box paired",
        "The Box took its welcome and this phone saved the link to it, with the counters the settings name.",
        Who.OWNER,
        "The link starts now. Keep Orbot running if the Box is a .onion address.",
    )

    /** The Box took its welcome but the settings name no counter, so there is no link to save yet. */
    val pairedNoCounters = w(
        Tones.INFO, "Box paired, no counters",
        "The Box took its welcome. A link needs a counter, and the settings name none, so no link was saved.",
        Who.OWNER,
        "Add a counter, then add the lines below to the setup text.",
    )

    /** The Box took its welcome but the link could not be saved. */
    val linkNotSaved = w(
        Tones.STOP, "Link not saved",
        "The Box took its welcome, but this phone could not save the link to it.",
        Who.OWNER,
        "Add the lines below to the setup text, then start the app again.",
    )

    /** The PIN is asked for to open the locked setup and lock it again with the link in it. */
    val savePin = w(
        Tones.GOOD, "Save the link",
        "The Box took its welcome. The link to it is saved in the locked setup, so the PIN is needed to open it and lock it again.",
        Who.OWNER,
        "Type the PIN of this phone, then save the link.",
    )

    /** A Box that was not handed its welcome was taken out of the queue. */
    val forgotten = w(
        Tones.INFO, "Box forgotten",
        "The Box's claim and the settings that add it were taken out of the queue. The Box itself was not told.",
        Who.OWNER,
        "To add it again, make a new invitation on the Key app and connect the Box again.",
    )

    fun all(): List<Pair<String, SayWords>> = listOf(
        "boxAddressNotRead" to addressNotRead,
        "boxLooking" to looking,
        "boxPriceNeeded" to priceNeeded,
        "boxCompare" to compare,
        "boxQueued" to queued,
        "boxHanding" to handing,
        "boxHandoverKept" to handoverKept,
        "boxPaired" to paired,
        "boxPairedNoCounters" to pairedNoCounters,
        "boxLinkNotSaved" to linkNotSaved,
        "boxSavePin" to savePin,
        "boxForgotten" to forgotten,
    )
}
