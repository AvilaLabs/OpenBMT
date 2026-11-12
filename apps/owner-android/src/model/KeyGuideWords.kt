package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The words of the key maker guide, shown after "Help me make a cold wallet" and the why screen: the phone
 * walks beside the laptop, and its step numbers match the key maker page's from step 4 on. They are written down
 * once and the screens copy them; nothing is written on a screen. `{n}` and `{total}` are the step and the count.
 * The list is written to docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object KeyGuideWords {
    const val kgTitle = "Make your cold wallet"
    const val kgStepBadge = "Step {n} of {total}"
    const val kgNext = "Next"
    const val kgBack = "Back"

    const val kgNeedTitle = "What you need"
    val kgNeedList = listOf(
        "A laptop or desktop computer. It does not have to be new.",
        "Two USB sticks: one of 8 GB or more for Tails, and any small one for the key maker page.",
        "A coin or one six-sided die, a pen, and the blank card from this app.",
        "About an hour, somewhere nobody can see your screen.",
    )
    const val kgNeedCard = "No card? Any paper will do for now. Write \"TEMPORARY. PUT THIS ON STEEL.\" at the top."

    const val kgTailsTitle = "Put Tails on the big stick"
    const val kgTailsText = "Tails is a free system that starts the computer from the USB stick, never touches its own disk, " +
        "and forgets everything when you shut down. On any computer, go to tails.net and follow its install guide for a USB stick. " +
        "Check the download the way the guide says."
    const val kgTailsNote = "This step needs the internet. The rest does not."

    const val kgPageTitle = "Put the key maker page on the small stick"
    const val kgPageText = "On the same computer, download the OpenBMT key maker page and its checksum file from the OpenBMT " +
        "releases page, and copy the page onto the small stick."
    const val kgPageCheck = "To check it, compare its SHA-256 with the checksum file. The handbook shows how."

    const val kgStartTitle = "Start Tails, offline"
    const val kgStartText = "Plug in the big stick and start the computer from it. On the Tails start screen, press + under " +
        "Additional Settings, choose Offline Mode, and turn it on. Then Start Tails."
    const val kgStartNote = "With Offline Mode on, Tails turns off the network before anything else runs."

    const val kgOpenTitle = "Open the key maker page"
    const val kgOpenText = "Plug in the small stick. Open it in the Files app and open openbmt-keymaker.html in Tor Browser. " +
        "If it says the page did not start, reload it once."

    const val kgFollowTitle = "Follow the page"
    const val kgFollowText = "Do the rehearsal first, then flip or roll for real. Come back to this phone when the page shows a code to scan."
    const val kgScanButton = "Scan the page's code"

    const val kgCompareTitle = "Compare with the page"
    const val kgCompareText = "The page and this phone must show the same fingerprint and first address, every character."
    const val kgFingerprint = "Fingerprint"
    const val kgFirstAddress = "First address"
    const val kgCompareSame = "They are the same"
    const val kgCompareDifferent = "They are different"
    const val kgDifferentText = "Stop. Do not use this wallet. On the page, press Clear and close, shut down Tails, and start again."

    const val kgElectrumTitle = "Check with Electrum"
    const val kgElectrumText = "Now follow step 6 on the page. Electrum must show the same fingerprint and first address as this phone."
    const val kgElectrumSame = "Electrum shows the same"
    const val kgElectrumDifferent = "Electrum shows something different"

    const val kgDoneTitle = "Shut down Tails"
    const val kgDoneText = "On the page, press Clear and close, then shut down Tails and take out both sticks. Keep the card " +
        "somewhere safe and private, and put the words on steel as soon as you can."
    const val kgDoneButton = "Done"

    const val kgStopTitle = "Stop"
    const val kgStartAgain = "Start again"

    const val kgNotKeyMakerHeading = "Not the key maker's code"
    const val kgNotKeyMakerWhat = "This code is for a wallet with more than one key. The key maker makes a wallet with one key."
    const val kgNotKeyMakerNext = "Scan the code on the key maker page."

    const val kgCameraOffHeading = "The camera is off for OpenBMT"
    const val kgCameraOffWhat = "The guide reads the page's code with the camera."
    const val kgCameraOffNext = "Allow the camera for OpenBMT in the phone's settings, then press Scan the page's code again."

    /** A scanned wallet with more than one key: shown on step 6, which stays. */
    val kgNotKeyMaker = SayWords(Tones.STOP, Who.OWNER, kgNotKeyMakerHeading, kgNotKeyMakerWhat, kgNotKeyMakerNext, null, null, null, null)

    /** The camera is off or did not start: shown on step 6. */
    val kgCameraOff = SayWords(Tones.STOP, Who.OWNER, kgCameraOffHeading, kgCameraOffWhat, kgCameraOffNext, null, null, null, null)

    /** The step badge for step [n] of [total]. */
    fun badge(n: Int, total: Int): String = kgStepBadge.replace("{n}", n.toString()).replace("{total}", total.toString())

    /** Every key and its text, in the order of the words file, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        fun add(key: String, text: String) { out.add(key to text) }
        add("kgTitle", kgTitle); add("kgStepBadge", kgStepBadge); add("kgNext", kgNext); add("kgBack", kgBack)
        add("kgNeedTitle", kgNeedTitle)
        kgNeedList.forEachIndexed { i, t -> add("kgNeedList${i + 1}", t) }
        add("kgNeedCard", kgNeedCard)
        add("kgTailsTitle", kgTailsTitle); add("kgTailsText", kgTailsText); add("kgTailsNote", kgTailsNote)
        add("kgPageTitle", kgPageTitle); add("kgPageText", kgPageText); add("kgPageCheck", kgPageCheck)
        add("kgStartTitle", kgStartTitle); add("kgStartText", kgStartText); add("kgStartNote", kgStartNote)
        add("kgOpenTitle", kgOpenTitle); add("kgOpenText", kgOpenText)
        add("kgFollowTitle", kgFollowTitle); add("kgFollowText", kgFollowText); add("kgScanButton", kgScanButton)
        add("kgCompareTitle", kgCompareTitle); add("kgCompareText", kgCompareText); add("kgFingerprint", kgFingerprint)
        add("kgFirstAddress", kgFirstAddress); add("kgCompareSame", kgCompareSame)
        add("kgCompareDifferent", kgCompareDifferent); add("kgDifferentText", kgDifferentText)
        add("kgElectrumTitle", kgElectrumTitle); add("kgElectrumText", kgElectrumText)
        add("kgElectrumSame", kgElectrumSame); add("kgElectrumDifferent", kgElectrumDifferent)
        add("kgStopTitle", kgStopTitle); add("kgStartAgain", kgStartAgain)
        add("kgNotKeyMakerHeading", kgNotKeyMakerHeading); add("kgNotKeyMakerWhat", kgNotKeyMakerWhat)
        add("kgNotKeyMakerNext", kgNotKeyMakerNext)
        add("kgCameraOffHeading", kgCameraOffHeading); add("kgCameraOffWhat", kgCameraOffWhat)
        add("kgCameraOffNext", kgCameraOffNext)
        add("kgDoneTitle", kgDoneTitle); add("kgDoneText", kgDoneText); add("kgDoneButton", kgDoneButton)
        return out
    }
}
