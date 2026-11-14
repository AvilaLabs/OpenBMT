package org.openbmt.owner.app

import org.openbmt.app.CameraWords
import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.owner.OwnerException
import org.openbmt.owner.ShopCurrency

/**
 * The words of the owner app's own states and refusals (docs/SCREEN-RULES.md).
 * The library gives words for everything its node knows (the coins, the
 * payments, the queue, the link, the old addresses, every refusal of a rule:
 * `docs/screens/owner.md`, `docs/screens/owner-node.md`, and the words the
 * `openbmt-owner-ffi` binding carries); these are the words of what the app
 * itself decides: setting up, Orbot, what was typed, what was copied or
 * saved, and its own faults. The list is written to docs/screens/owner-app.md
 * by the JVM test, and every one of them passes the checks of
 * `openbmt_core::say::check` as far as text can be checked.
 */
object AppWords {
    private fun w(tone: Int, heading: String, what: String, who: Int, next: String, detail: String? = null) =
        SayWords(tone, who, heading, what, next, null, null, null, detail)

    private fun detailOf(problem: String?): String? = problem?.let { Format.cleanForeign(it) }?.takeIf { it.isNotEmpty() }

    // ---- watching in the background (OA-80 to OA-89) ------------------------------------------

    /** The setting's row. The second sentence of what it does is [watchSettingMore]: the words check keeps `what` to 140 characters. */
    val watchSetting = w(
        Tones.INFO, "Watch in the background",
        "The phone keeps following the shop's wallet while the app is closed, and tells the counters it is watching.",
        Who.OWNER,
        "Turn it on to keep instant sales working. Android shows a notification while it runs.",
    )

    /** The rest of what the setting does, shown under [watchSetting]. */
    const val watchSettingMore = "Without this, instant sales stop about ten minutes after you close the app."

    val watchNotificationsOff = w(
        Tones.STOP, "Notifications are off",
        "Android needs to show a notification while the phone watches in the background, and notifications are off for this app.",
        Who.OWNER,
        "Allow notifications for OpenBMT Owner in Android's settings, then turn this on again.",
    )

    /** The notification while it runs (OA-82). Not a screen: Android shows these. */
    const val watchNotificationTitle = "Watching the shop"
    const val watchNotificationText = "Instant sales stay on while this runs."
    const val watchNotificationStop = "Stop"

    /** The notification when Tor is not connected (OA-88). */
    const val watchNotificationNoOrbot = "Not watching: Tor is not connected. Instant sales are off."

    // ---- the key map on the key-setup screens ---------------------------------------------------

    /** The three keys' names, where each lives, and the state words, as the key map shows them. */
    const val keyMapKey1 = "Key 1"
    const val keyMapKey1Where = "This phone"
    const val keyMapKey2 = "Key 2"
    const val keyMapKey2Where = "Key app"
    const val keyMapKey3 = "Key 3"
    const val keyMapKey3Where = "On steel"
    const val keyMapMade = "Made"
    const val keyMapNext = "Next"
    const val keyMapNotYet = "Not yet"
    const val keyMadeTitle = "Your keys"
    const val keyMadeHeading = "Now make key 2"
    const val keyMadeText = "Open the Key app on the second phone and choose Set up keys. It shows a code. Scan that code with this phone."
    const val keyMapRule = "Any 2 of the 3 keys can spend. One alone cannot."

    /** The key map's spoken description; `{k1}`, `{k2}` and `{k3}` are the state words in lower case. */
    const val keyMapSpoken = "Key 1, on this phone: {k1}. Key 2, on the Key app: {k2}. Key 3, on steel: {k3}. Any 2 of the 3 keys can spend."

    /** Every key and its text, in the order of the words file, for docs/screens/owner-app.md. */
    fun keyMapAll(): List<Pair<String, String>> = listOf(
        "keyMapKey1" to keyMapKey1, "keyMapKey1Where" to keyMapKey1Where,
        "keyMapKey2" to keyMapKey2, "keyMapKey2Where" to keyMapKey2Where,
        "keyMapKey3" to keyMapKey3, "keyMapKey3Where" to keyMapKey3Where,
        "keyMapMade" to keyMapMade, "keyMapNext" to keyMapNext, "keyMapNotYet" to keyMapNotYet,
        "keyMapRule" to keyMapRule, "keyMapSpoken" to keyMapSpoken,
        "keyMadeTitle" to keyMadeTitle, "keyMadeHeading" to keyMadeHeading, "keyMadeText" to keyMadeText,
    )

    // ---- setting up ------------------------------------------------------------------

    val notSetUp = w(
        Tones.INFO, "Not set up yet",
        "This phone has not been set up as the shop's owner phone yet.",
        Who.OWNER,
        "Make a new shop wallet with the Key app, or use setup text you already have.",
    )

    /** The screen of the second choice: setup text from a key ceremony made elsewhere. */
    val setupTextNeeded = w(
        Tones.INFO, "Setup text needed",
        "This phone takes the setup text of a key ceremony made elsewhere, pasted or from a file.",
        Who.OWNER,
        "Paste the setup text, or choose its file, then save.",
    )

    val setupSaved = w(
        Tones.GOOD, "Owner phone set up",
        "The setup is saved on this phone, sealed with a key that stays in the phone.",
        Who.OWNER,
        "Keep the app open. It starts looking for the network now.",
    )

    fun setupNotReadable(problem: String?) = w(
        Tones.STOP, "Setup text not read",
        "The setup text is not in the form this app reads, so nothing was saved.",
        Who.OWNER,
        "Copy the setup text again from the key ceremony, then try again.",
        detailOf(problem),
    )

    val torAddressNotAccepted = w(
        Tones.STOP, "Tor address not accepted",
        "The address of Orbot must be a host, a colon and a port, like 127.0.0.1:9050.",
        Who.OWNER,
        "Type the address again.",
    )

    val torAlwaysOn = w(
        Tones.INFO, "Everything goes through Tor",
        "This app sends every connection through Tor, with Orbot, and has no way to turn that off.",
        Who.OWNER,
        "Check the address of Orbot below, and keep Orbot running.",
    )

    // ---- Tor and Orbot ------------------------------------------------------------------

    /** The built-in Tor, which is the choice of a new setup (OA-97, OA-99). */
    val torBuiltIn = w(
        Tones.INFO, "Tor",
        "This app runs Tor itself, so Orbot is not needed.",
        Who.NOBODY,
        "Nothing to do. To use Orbot instead, choose Use Orbot.",
    )

    val torUsesOrbot = w(
        Tones.INFO, "Tor through Orbot",
        "This app sends everything through Orbot, which must be installed and running.",
        Who.OWNER,
        "Keep Orbot running. To stop needing it, choose Built-in Tor.",
    )

    val torStarting = w(
        Tones.INFO, "Starting Tor",
        "This app is connecting to the Tor network, which can take up to a minute.",
        Who.NOBODY,
        "Wait on this screen.",
    )

    val torFailed = w(
        Tones.STOP, "Tor did not start",
        "This app could not connect to the Tor network.",
        Who.OWNER,
        "Check that the phone is online, then try again. If it keeps failing, choose Use Orbot.",
    )

    val orbotNotRunning = w(
        Tones.STOP, "Tor is not running",
        "This app sends everything through Tor, but Orbot is not running.",
        Who.OWNER,
        "Open Orbot and turn it on.",
    )

    // ---- what was typed -------------------------------------------------------------------

    val notANumber = w(
        Tones.STOP, "Check what you typed",
        "An amount, a number or a rate is empty or has something other than digits in it.",
        Who.OWNER,
        "Type whole numbers only, then try again.",
    )

    val counterNeeded = w(
        Tones.INFO, "Counter needed",
        "A refund is asked for on one counter's sale, and no counter is chosen.",
        Who.OWNER,
        "Choose the counter, or paste its identity of 64 digits, then try again.",
    )

    val counterNotAccepted = w(
        Tones.STOP, "Counter identity not read",
        "A counter's identity is exactly 64 digits and letters from a to f.",
        Who.OWNER,
        "Type the identity again, or choose the counter from the list.",
    )

    val responseNeeded = w(
        Tones.INFO, "Response needed",
        "The Key app's response is needed to finish the approval.",
        Who.OWNER,
        "Paste the response or choose its file, then press the button again.",
    )

    fun responseNotReadable(problem: String?) = w(
        Tones.STOP, "Response text not read",
        "The text is not in the form this app reads, so the Key app's response was not used.",
        Who.OWNER,
        "Copy the response again from the Key app, or choose its file instead.",
        detailOf(problem),
    )

    // ---- the approval as a code -------------------------------------------------------------

    /** The request is on the screen as a code that changes. */
    val requestCodeAnimated = w(
        Tones.INFO, "Request shown as a code",
        "The request is on the screen as a code that changes, one part after another.",
        Who.OWNER,
        "On the Key app, choose Scan and point it at this code until it has read every part. Then press Done.",
    )

    /** The request is on the screen as one code. */
    val requestCodeStill = w(
        Tones.INFO, "Request shown as one code",
        "The request is on the screen as one code that does not change.",
        Who.OWNER,
        "On the Key app, choose Scan and point it at this code. Then press Done.",
    )

    /** No code could be made of the request. */
    val requestCodeNotMade = w(
        Tones.STOP, "No code could be made",
        "The request could not be turned into a code, so nothing is shown.",
        Who.OWNER,
        "Go back and save the request as a file, or copy it as text, instead.",
    )

    /** A scan ended with no code read: cancelled, or the permission dialog was dismissed. */
    val scanCancelled = w(
        Tones.INFO, "Scan cancelled",
        "No code was read, so nothing was taken from the Key app.",
        Who.OWNER,
        "Scan the Key app's code again, or choose its file or paste its text instead.",
    )

    /** A single code was read where the response is a code that changes. */
    val scanNotAResponse = w(
        Tones.STOP, "Not the Key app's code",
        "The code read is not one the Key app shows for a response, so nothing was used.",
        Who.OWNER,
        "On the Key app, show the answer as a code, and scan that one.",
    )

    // ---- adding a counter -------------------------------------------------------------------

    /** The request of a counter is being read; the four words take a moment to work out. */
    val counterReading = w(
        Tones.INFO, "Reading the counter",
        "The counter's request is being read and its four words worked out, which takes a moment.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** A single code was read where a counter's request is a code that may change. */
    val counterNotACode = w(
        Tones.STOP, "Not a counter's code",
        "The code read is not one a counter shows for its request, so nothing was queued.",
        Who.OWNER,
        "On the counter, choose Pair and scan the code it shows.",
    )

    /** The scan of a counter's code was cancelled, or its permission dialog was dismissed. */
    val counterScanCancelled = w(
        Tones.INFO, "Counter scan cancelled",
        "No code was read, so nothing was queued.",
        Who.OWNER,
        "Scan the counter's code again when you are ready.",
    )

    /** A complete code that is longer than a counter's request can be. */
    val counterCodeNotRequest = w(
        Tones.STOP, "Not a counter's request",
        "The code holds more than a counter's request can, so nothing was queued.",
        Who.OWNER,
        "On the counter, choose Pair and scan the code it shows.",
    )

    /** The shares of the instant limit once the counter is added: equal; the settings editor changes them. */
    fun sharesNote(counters: Int) = w(
        Tones.INFO, "Equal shares",
        if (counters <= 1) {
            "This is the only counter, so its share of the instant limit is all of it."
        } else {
            "Each counter's share of the instant limit becomes 1/$counters."
        },
        Who.OWNER,
        "Custom shares are set under Settings, in Change settings.",
    )

    /** What the owner does with the four words. */
    val counterCompare = w(
        Tones.INFO, "Compare the four words",
        "Check that the counter shows these same four words, and that the Key app shows them too.",
        Who.OWNER,
        "If they match, say so below. If they differ, say so and nothing is paired.",
    )

    /** A phone with no settings needs today's price of a bitcoin to start them. */
    val counterPriceNeeded = w(
        Tones.INFO, "Price needed",
        "This phone holds no settings yet, and the starting limits are in dollars, so it needs today's price of a bitcoin.",
        Who.OWNER,
        "Type today's price of one bitcoin in whole dollars, then say the words match.",
    )

    /** The phone is asking the price sources for the price, while a phone with no settings waits for it (OA-101). */
    val counterPriceFetching = w(
        Tones.INFO, "Getting today's price",
        "This phone is asking the price sources for the price of one bitcoin, through Tor.",
        Who.NOBODY,
        "Wait a moment. If they do not answer, you can type the price.",
    )

    /** The sources agreed, and the price is in the field (OA-101). */
    fun counterPriceFetched(price: Long) = w(
        Tones.INFO, "Today's price",
        "The price sources give ${PriceModel.dollars(price)} dollars for one bitcoin. The starting limits are worked out from it.",
        Who.OWNER,
        "If it looks right, say the words match. To use another price, type it below.",
    )

    /** The price typed is not a whole number of dollars above 0. */
    val priceNotRead = priceNotReadOf(ShopCurrency.Usd)

    /** The same in the currency the price is asked in; the currency's name stands where "dollars" stood. */
    fun priceNotReadIn(c: ShopCurrency): SayWords = if (c == ShopCurrency.Usd) priceNotRead else priceNotReadOf(c)

    private fun priceNotReadOf(c: ShopCurrency): SayWords = w(
        Tones.STOP, "Price not read",
        "The price of a bitcoin is a whole number of ${c.plural} above 0, so nothing was queued.",
        Who.OWNER,
        "Type today's price of one bitcoin in whole ${c.plural}, like 98500.",
    )

    /** The owner said the words differ, and the queued request was taken out. */
    val counterCancelled = w(
        Tones.INFO, "Counter not added",
        "The four words did not match, so the counter's request was taken out of the queue.",
        Who.OWNER,
        "Start again on the counter, and scan the new code it shows.",
    )

    /** The Key app did not approve: nothing waits for the counter. */
    val counterNotPaired = w(
        Tones.STOP, "Counter not paired",
        "The Key app did not approve this counter, so no code for it was made.",
        Who.OWNER,
        "Read the words above, then scan the counter's code again to start over.",
    )

    /** The code for the counter is on the screen with the first addresses in it, and it changes. */
    val counterCodeAnimated = w(
        Tones.INFO, "Code for the counter",
        "The code for the counter is on the screen in many parts, with its first addresses. Taking it takes about a minute.",
        Who.OWNER,
        "Scan this with the counter and hold both phones steady. The counter counts the parts it has and " +
            "shows Paired when it has them all.",
    )

    /** The code for the counter is on the screen as one code. */
    val counterCodeStill = w(
        Tones.INFO, "Code for the counter",
        "The code for the counter is on the screen as one code that does not change.",
        Who.OWNER,
        "Scan this with the counter. It shows Paired when it has taken it.",
    )

    /** The code has no first addresses: they were handed over already. */
    val counterCodeNoBatch = w(
        Tones.INFO, "Code without addresses",
        "The first addresses are not in this code, because the link handed them over already.",
        Who.OWNER,
        "Scan this with the counter. It shows Paired when it has taken it.",
    )

    /** No code could be made of the bundle. */
    val counterCodeNotMade = w(
        Tones.STOP, "No code could be made",
        "The code for the counter could not be made, so nothing is shown. What waits for the counter is kept.",
        Who.OWNER,
        "Try to show it again.",
    )

    /** The owner says the counter shows Paired. */
    val counterDelivered = w(
        Tones.GOOD, "Counter paired",
        "The counter shows Paired, so what waited for it is marked delivered.",
        Who.OWNER,
        "The counter takes its sales from its first addresses now.",
    )

    // ---- making a new shop wallet ---------------------------------------------------------------

    /** Key 1 is being made; it takes a moment. */
    val ceremonyMakingKey = w(
        Tones.INFO, "Making key 1",
        "Key 1 is being made on this phone, which takes a moment.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** Key 1 is made and the answer of the Key app is next. */
    val ceremonyScanNext = w(
        Tones.INFO, "Scan the Key app",
        "Open the Key app on the other phone and choose Set up keys.",
        Who.OWNER,
        "Press the button below and scan the code it shows.",
    )

    /** A single code was read where the Key app's answer is a code that may change. */
    val ceremonyNotAnAnswer = w(
        Tones.STOP, "Not the Key app's answer",
        "The code read is not one the Key app shows for its answer, so nothing was used.",
        Who.OWNER,
        "On the Key app, choose Set up keys, and scan the code it shows.",
    )

    /** A complete code that is longer than the Key app's answer can be. */
    val ceremonyCodeTooLong = w(
        Tones.STOP, "Code too long for an answer",
        "The code holds more than the Key app's answer can, so nothing was used.",
        Who.OWNER,
        "On the Key app, choose Set up keys, and scan the code it shows.",
    )

    /** The scan of the Key app's answer was cancelled, or its permission dialog was dismissed. */
    val ceremonyScanCancelled = w(
        Tones.INFO, "Key app scan cancelled",
        "No code was read, so no answer was taken from the Key app.",
        Who.OWNER,
        "Scan the Key app's code again when you are ready.",
    )

    /** What the owner does with the Key app's four words. */
    val ceremonyCompareRoot = w(
        Tones.INFO, "Compare the Key app's words",
        "Check that the Key app shows these same four words as its own.",
        Who.OWNER,
        "If they match, say so below. If they differ, say so and the answer is thrown away.",
    )

    /** The owner said the Key app's words differ, and the answer was thrown away. */
    val ceremonyRootDiffers = w(
        Tones.STOP, "Answer thrown away",
        "The four words differ, so this answer may not come from the Key app you hold, and it was thrown away.",
        Who.OWNER,
        "Scan again on the Key app you hold. If the words still differ, choose Start again.",
    )

    /** The Key app holds no key 2: it comes from the device that does. */
    val ceremonyKey2Needed = w(
        Tones.INFO, "Key 2 needed",
        "The Key app holds no key 2, so key 2 comes from the device that holds it.",
        Who.OWNER,
        "Scan a key code from the other wallet app, or its key as text, or paste the text, then continue.",
    )

    /** The text of key 2 is empty or longer than a key's text can be. */
    val ceremonyKey2Text = w(
        Tones.STOP, "Key 2 text not accepted",
        "The text for key 2 is empty, is not text, or is longer than a key's text can be, so nothing was used.",
        Who.OWNER,
        "Copy it again from the device that holds key 2, then try again.",
    )

    /** The scan of key 2 was cancelled, or its permission dialog was dismissed. */
    val ceremonyKey2ScanCancelled = w(
        Tones.INFO, "Key 2 scan cancelled",
        "No code was read, so key 2 was not taken.",
        Who.OWNER,
        "Scan its code again, or paste its text instead.",
    )

    /** The wallet is being built; it takes a moment. */
    val ceremonyBuilding = w(
        Tones.INFO, "Building the wallet",
        "The wallet is being built from the three keys, which takes a moment.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** The wallet is built and its words are on the screen. */
    val ceremonyCheckWallet = w(
        Tones.INFO, "Check the wallet",
        "The checksum words and the fingerprints below are the wallet's. The Key app shows the same ones.",
        Who.OWNER,
        "Show the Key app its setup next, and compare there.",
    )

    /** The setup for the Key app is on the screen as a code that changes. */
    val ceremonyCodeAnimated = w(
        Tones.INFO, "Setup for the Key app",
        "The setup for the Key app is on the screen as a code that changes, one part after another.",
        Who.OWNER,
        "On the Key app, scan this code until it has read every part, then compare the words.",
    )

    /** The setup for the Key app is on the screen as one code. */
    val ceremonyCodeStill = w(
        Tones.INFO, "Setup for the Key app",
        "The setup for the Key app is on the screen as one code that does not change.",
        Who.OWNER,
        "On the Key app, scan this code, then compare the words.",
    )

    /** No code could be made of the setup. */
    val ceremonyCodeNotMade = w(
        Tones.STOP, "No code could be made",
        "The setup for the Key app could not be turned into a code, so nothing is shown.",
        Who.OWNER,
        "Go back and try to show it again.",
    )

    /** The owner compares the wallet's words with the Key app's. */
    val ceremonyCompareSetup = w(
        Tones.INFO, "Compare with the Key app",
        "After it takes the setup, the Key app shows the checksum words and the fingerprints.",
        Who.OWNER,
        "Check that they are the same as on this phone, then say so below.",
    )

    /** The owner says the words differ: the wallet is not finished. */
    val ceremonyWordsDiffer = w(
        Tones.STOP, "Wallet not finished",
        "The words differ, so the wallet is not finished, and nothing should be paid to it.",
        Who.OWNER,
        "Choose Start again on both phones.",
    )

    /** The wallet was made on a phone that did not keep the setup for the Key app. */
    val ceremonyScanAgain = w(
        Tones.INFO, "Scan the Key app again",
        "The wallet was built, but the setup for the Key app is not kept on this phone.",
        Who.OWNER,
        "Scan the Key app's code again to show the setup again.",
    )

    /** The owner confirmed, and the setup is being saved. */
    val ceremonyFinishing = w(
        Tones.INFO, "Finishing the setup",
        "The setup is being finished and saved on this phone.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** A finished setup waits to be saved. */
    val ceremonySaveNext = w(
        Tones.INFO, "Save the setup",
        "The words were confirmed, but the setup was not saved on this phone yet.",
        Who.OWNER,
        "Press the button below to save it and start.",
    )

    /** An unfinished setup was found at the start. */
    val ceremonyResume = w(
        Tones.INFO, "Setup in progress",
        "A new shop wallet was started on this phone and is not finished.",
        Who.OWNER,
        "Continue where it stopped, or start again.",
    )

    /** The record of the unfinished setup does not read. */
    fun ceremonyRecordNotRead(problem: String?) = w(
        Tones.STOP, "Setup record not read",
        "The record of the unfinished setup could not be read, so nothing was done.",
        Who.OWNER,
        "Choose Start again and make the wallet again. Nothing has been paid to it.",
        detailOf(problem),
    )

    /** The question before the unfinished setup is thrown away. */
    val ceremonyStartAgain = w(
        Tones.STOP, "Start again?",
        "The keys made so far are thrown away, and nothing has been paid to them.",
        Who.OWNER,
        "Choose Start again only if the Key app starts again too.",
    )

    /** The unfinished setup was thrown away. */
    val ceremonyThrownAway = w(
        Tones.INFO, "Started over",
        "The keys made so far were thrown away, and nothing was paid to them.",
        Who.OWNER,
        "Make a new shop wallet again, or use setup text you already have.",
    )

    // ---- what was done --------------------------------------------------------------------

    val requestReady = w(
        Tones.GOOD, "Request ready",
        "The request for the Key app is ready, and nothing has been signed yet.",
        Who.OWNER,
        "Show it as a code, or save it as a file or copy it as text, give it to the Key app, then bring back its response.",
    )

    val requestCopied = w(
        Tones.INFO, "Request copied",
        "The request is copied as text, written in lower case.",
        Who.OWNER,
        "Paste it where the Key app can read it.",
    )

    fun requestSaved(name: String) = w(
        Tones.INFO, "Request saved",
        "The request is saved as a file.",
        Who.OWNER,
        "Carry the file to the Key app and open it there.",
        detailOf(name),
    )

    val refundQueuedAtDepth = w(
        Tones.GOOD, "Refund queued",
        "The refund is in the queue, and the sale is deep enough to be refunded.",
        Who.OWNER,
        "Bring this phone and the Key app together and start an approval.",
    )

    val refundQueuedWaiting = w(
        Tones.INFO, "Refund queued, sale new",
        "The refund is in the queue, but the sale is not deep enough yet to be refunded.",
        Who.OWNER,
        "Wait for more blocks. The refund can be approved when the sale is deep enough.",
    )

    val cashOutQueued = w(
        Tones.GOOD, "Cash-out queued",
        "The cash-out is in the queue and has not been sent for approval yet.",
        Who.OWNER,
        "Bring this phone and the Key app together and start an approval.",
    )

    /** The sources gave the price a cash-out is recorded at, in the shop's currency (OA-102). */
    fun cashOutPrice(whole: Long, c: ShopCurrency) = w(
        Tones.INFO, "Today's price",
        "The price sources give ${PriceModel.dollars(whole)} ${c.plural} for one bitcoin. The books record this cash-out at that price.",
        Who.OWNER,
        "If it looks right, go on. To use another price, type it below.",
    )

    /** The sources did not answer, so the owner types the price a cash-out is recorded at (OA-102). */
    val cashOutPriceNeeded = w(
        Tones.INFO, "Price needed",
        "The price sources did not answer, and the books need what this cash-out is worth.",
        Who.OWNER,
        "Type the price of one bitcoin, then go on.",
    )

    /** No counter has reported a sale (OA-103). */
    val booksEmpty = w(
        Tones.INFO, "No sales yet",
        "No counter has sent a sale to this phone yet.",
        Who.NOBODY,
        "Nothing to do. Sales appear here as the counters report them.",
    )

    /** A file of the books was written where the owner chose (OA-103). */
    val booksSaved = w(
        Tones.GOOD, "File saved",
        "The file is saved where you chose. It holds sale numbers, times and amounts, and no addresses.",
        Who.OWNER,
        "Give it to your bookkeeper.",
    )

    val entryCancelled = w(
        Tones.INFO, "Entry cancelled",
        "The entry is taken out of the queue.",
        Who.NOBODY,
        "Nothing to do.",
    )

    val entryRequeued = w(
        Tones.INFO, "Entry queued again",
        "The entry that was sent back is in the queue again.",
        Who.OWNER,
        "Start an approval when the Key app is at hand.",
    )

    val sessionAbandoned = w(
        Tones.INFO, "Approval abandoned",
        "The open approval is closed, and its entries are back in the queue.",
        Who.NOBODY,
        "Nothing to do. Start a new approval when the Key app is at hand.",
    )

    /** The question before an approval that went to a hardware signer is abandoned (AS-89). */
    val abandonExported = w(
        Tones.STOP, "Already sent to the signer",
        "This approval went to the signer, so it may already be signed and could still be sent.",
        Who.OWNER,
        "If you abandon it, the payments go back in the queue and a new approval spends the same coins, so only one can ever be paid.",
    )

    /** The rest of what to do next, shown under [abandonExported]: the words check keeps each sentence to 140 characters. */
    const val abandonExportedMore = "If the old one is sent, it counts as paid."

    /** An approval that went to a hardware signer was abandoned (AS-89). */
    val abandonedExported = w(
        Tones.INFO, "Back in the queue",
        "The payments are back in the queue. The payment sent to the signer is still watched: if it is sent, it counts as paid.",
        Who.OWNER,
        "Start a new approval when you are ready.",
    )

    val alertCleared = w(
        Tones.INFO, "Alert cleared",
        "The alert is cleared from this phone.",
        Who.NOBODY,
        "Nothing to do. If the cause is still there, the alert comes back.",
    )

    val noteDismissed = w(
        Tones.INFO, "Note dismissed",
        "The note about a refund request is removed from the list.",
        Who.NOBODY,
        "Nothing to do.",
    )

    // ---- what the node does not hold ---------------------------------------------------------

    val noLink = w(
        Tones.INFO, "No link to the counters",
        "This phone was set up without a relay, so no counter can report to it.",
        Who.OWNER,
        "Nothing to do here. A setup text with a relay adds the link.",
    )

    val noSettingsYet = w(
        Tones.INFO, "No settings yet",
        "The phone holds no settings for the shop yet, so there is nothing to show.",
        Who.OWNER,
        "Nothing to do. They arrive with the first approval that carries settings.",
    )

    // ---- changing the settings (OA-32 to OA-35) -----------------------------------------------

    /** The first screen of the editor: what the steps are. */
    val editIntro = w(
        Tones.INFO, "Change the settings",
        "Pick a group, change what you need, then review. Nothing is queued until you press Queue the change.",
        Who.OWNER,
        "A change that loosens something needs the Key app. A stricter change does not.",
    )

    val editGroupCurrency = w(
        Tones.INFO, "Currency",
        "The money the counters price sales in, and the limits are set in.",
        Who.OWNER,
        "Choose one. Bitcoin Standard prices in sats and uses no price source.",
    )

    /** Shown while a new currency is chosen: the limits are typed again. */
    val editCurrencyRetype = w(
        Tones.INFO, "New currency, new limits",
        "Every limit is set again in the new currency, so the change shows them empty.",
        Who.OWNER,
        "Type each limit in the new currency, then review. The change needs the Key app.",
    )

    val editGroupInstant = editGroupInstantOf(ShopCurrency.Usd)

    fun editGroupInstantIn(c: ShopCurrency): SayWords = if (c == ShopCurrency.Usd) editGroupInstant else editGroupInstantOf(c)

    private fun editGroupInstantOf(c: ShopCurrency): SayWords = w(
        Tones.INFO, "Instant sales",
        "How much a counter may accept as paid before the payment is confirmed, for each way it checks.",
        Who.OWNER,
        "Whole ${c.plural}. 0 turns a limit off.",
    )

    val editGroupPayments = w(
        Tones.INFO, "Payments",
        "When a payment counts, how many blocks settle a sale, and the smallest bitcoin sale.",
        Who.OWNER,
        "Change what you need, then press Done.",
    )

    val editGroupTips = w(
        Tones.INFO, "Tips",
        "Whether the counters ask the customer for a tip, and which tips they suggest.",
        Who.OWNER,
        "Turn tips on, and set up to three percents.",
    )

    val editGroupRefunds = editGroupRefundsOf(ShopCurrency.Usd)

    fun editGroupRefundsIn(c: ShopCurrency): SayWords = if (c == ShopCurrency.Usd) editGroupRefunds else editGroupRefundsOf(c)

    private fun editGroupRefundsOf(c: ShopCurrency): SayWords = w(
        Tones.INFO, "Refunds and overrides",
        "How much can be refunded, and how much cashiers and managers may accept against a red screen.",
        Who.OWNER,
        "Whole ${c.plural}. 0 turns a limit off.",
    )

    val editGroupSafety = w(
        Tones.INFO, "Safety",
        "How recent this phone's report must be, and what the shop says about its network.",
        Who.OWNER,
        "Change what you need, then press Done.",
    )

    val editGroupShares = w(
        Tones.INFO, "Counters' shares",
        "Each counter's share of every instant limit. The shares may add up to 100% at most.",
        Who.OWNER,
        "Type each share as a percent with up to two decimals.",
    )

    // ---- Lightning through a swap provider (CR-218) --------------------------------------------

    /** The settings row, and the heading of its screen. These lines have no heading of their own, so they are not in the list of words. */
    const val swapSettingTitle = "Lightning through a swap provider"
    const val swapSettingText =
        "Customers can pay over Lightning. A swap provider you pick turns each payment into bitcoin that only your counter can claim, to your own wallet. The provider never holds your money."
    const val swapSettingProvider = "Provider"
    const val swapSettingOnion = "Onion address"
    const val swapSettingMax = "Largest sale over Lightning"
    const val swapSettingOn = "Turn on"
    const val swapSettingOff = "Turn off"

    /** The provider's fee, once its live terms are known: the percentage and the miner fees in satoshis. */
    fun swapSettingFee(percent: String, sats: Long) = "The provider charges $percent% plus about $sats sats per sale."

    val editGroupLightning = w(
        Tones.INFO, "Lightning node",
        "The shop's own Lightning node: every Lightning payment must be made out to this node's key.",
        Who.OWNER,
        "Paste or scan the node's key or address, or remove the node.",
    )

    /** What naming a node needs. */
    val editLightningNote = w(
        Tones.INFO, "Naming a node",
        "Naming a node needs the Key app, which shows the key in full.",
        Who.OWNER,
        "Afterwards, set up Lightning on each counter, which holds its own limited key.",
    )

    val editNothingChanged = w(
        Tones.INFO, "Nothing changed",
        "Every value is what the settings hold now, so there is nothing to queue.",
        Who.OWNER,
        "Go back and change a value, or leave the editor.",
    )

    /** The review: some field loosens. */
    val editNeedsKeyApp = w(
        Tones.INFO, "The Key app must approve",
        "Something here loosens a limit or a check, so the whole change needs the Key app's approval.",
        Who.OWNER,
        "Queue the change, then build the request for the Key app.",
    )

    /** The review: every field tightens. */
    val editTakesEffect = w(
        Tones.INFO, "Takes effect when delivered",
        "Every change here makes things stricter, so this phone signs it alone and no approval is needed.",
        Who.OWNER,
        "Queue the change. It takes effect when it is delivered to the counters.",
    )

    val editQueuedLoosening = w(
        Tones.GOOD, "Change queued",
        "The change waits for the Key app's approval. It takes effect after the Key app approves and it is delivered.",
        Who.OWNER,
        "Build the request for the Key app, show it there, and bring its response back.",
    )

    val editQueuedTightening = w(
        Tones.GOOD, "Change signed",
        "The change is signed by this phone and waits to be delivered to the counters.",
        Who.OWNER,
        "Nothing more is needed. See it under Waiting to be delivered.",
    )

    /** The Lightning field was used with nothing in it. */
    val editNodeNeeded = w(
        Tones.STOP, "Node key needed",
        "The node's key or address was left empty, so no node was named.",
        Who.OWNER,
        "Paste or scan the key or address of the node, or choose Remove the node.",
    )

    /** There is no counter to give a share to. */
    val editNoCounters = w(
        Tones.INFO, "No counters yet",
        "The settings list no counters, so there are no shares to change.",
        Who.OWNER,
        "Add a counter first. Equal shares are made when it is added.",
    )

    /** A share was not a percent. */
    val editSharesNotPercent = w(
        Tones.STOP, "Share not accepted",
        "A share must be a percent from 0 to 100, with up to two decimals, so nothing was saved.",
        Who.OWNER,
        "Type each share again, like 50 or 33.33.",
    )

    /** A suggested tip was not a whole percent from 1 to 50. */
    val editTipNotRead = w(
        Tones.STOP, "Tip not accepted",
        "A suggested tip must be a whole percent from 1 to 50, so nothing was saved.",
        Who.OWNER,
        "Type it again, like 15, or leave it empty.",
    )

    /** What was typed for a field is not the kind of number it takes. */
    fun editValueNotRead(kind: FieldKind, label: String, c: ShopCurrency = ShopCurrency.Usd): SayWords {
        if (kind == FieldKind.TipPercent) return editTipNotRead
        val (what, next) = when (kind) {
            FieldKind.Dollars -> "is not a whole number of ${c.plural}" to "Type whole ${c.plural}, like 25, or 0 to turn it off."
            FieldKind.Percent -> "is not a percent" to "Type a percent with up to two decimals, like 0.5."
            FieldKind.Blocks -> "is not a whole number of blocks" to "Type a whole number of blocks, like 6."
            FieldKind.Minutes -> "is not a whole number of minutes" to "Type whole minutes, like 10."
            FieldKind.Sats -> "is not a whole number of satoshis" to "Type whole satoshis, like 1000, or 0 for none."
            FieldKind.Switch -> "was not chosen" to "Choose one of the two answers."
            FieldKind.TipPercent -> return editTipNotRead
        }
        return w(
            Tones.STOP, "Value not accepted",
            "The value for ${Format.cleanForeign(label)} $what, so nothing was saved.",
            Who.OWNER,
            next,
        )
    }

    /** The words of each field the editor changes: the label is the heading, then what it does, then how to type it. */
    val editFields: List<Pair<String, SayWords>> = editFieldsIn(ShopCurrency.Usd)

    /** The fields' words with the currency's name where "dollars" stood. */
    fun editFieldsIn(c: ShopCurrency): List<Pair<String, SayWords>> = listOf(
        "light_per_sale" to w(
            Tones.INFO, "Light check: one sale",
            "The largest sale accepted as instant when only a light check has seen the payment.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "light_budget" to w(
            Tones.INFO, "Light check: all at once",
            "The most that may wait for confirmation together across the shop, on a light check.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "node_per_sale" to w(
            Tones.INFO, "Own node: one sale",
            "The largest sale accepted as instant when the shop's own node has also seen the payment.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "node_budget" to w(
            Tones.INFO, "Own node: all at once",
            "The most that may wait for confirmation together across the shop, with the own node.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "box_per_sale" to w(
            Tones.INFO, "Box only: one sale",
            "The largest sale accepted as instant when only a Box reports the payment.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "box_budget" to w(
            Tones.INFO, "Box only: all at once",
            "The most that may wait for confirmation together across the shop, on a Box's word.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "underpayment_tolerance_bps" to w(
            Tones.INFO, "Underpayment still counted",
            "How far under the price a payment may fall and still count as paid.",
            Who.OWNER, "A percent with up to two decimals, from 0 to 5.",
        ),
        "min_confirmations" to w(
            Tones.INFO, "Blocks to settle a sale",
            "How many blocks a payment needs before the sale counts as settled.",
            Who.OWNER, "A whole number from 1 to 100.",
        ),
        "final_confirmations" to w(
            Tones.INFO, "Blocks until final",
            "How many blocks until a sale is final and a refund may be paid. It is at least the number above.",
            Who.OWNER, "A whole number from 1 to 100.",
        ),
        "min_sale_sats" to w(
            Tones.INFO, "Smallest bitcoin sale",
            "A bitcoin sale below this is turned away, because a tiny receipt can cost more to spend than it holds.",
            Who.OWNER, "Whole satoshis. 0 means no smallest sale.",
        ),
        "tips_on" to w(
            Tones.INFO, "Ask for a tip",
            editGroupTips.what,
            Who.OWNER, editGroupTips.next,
        ),
        "tip_1" to w(
            Tones.INFO, "First tip",
            editGroupTips.what,
            Who.OWNER, "A percent from 1 to 50, or empty.",
        ),
        "tip_2" to w(
            Tones.INFO, "Second tip",
            editGroupTips.what,
            Who.OWNER, "A percent from 1 to 50, or empty.",
        ),
        "tip_3" to w(
            Tones.INFO, "Third tip",
            editGroupTips.what,
            Who.OWNER, "A percent from 1 to 50, or empty.",
        ),
        "refund_cap" to w(
            Tones.INFO, "Largest single refund",
            "The most one refund may pay back to a customer.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "refund_daily_cap" to w(
            Tones.INFO, "Refunds per day",
            "The most that may be refunded in one day, all refunds together.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "cashier_override_daily_cap" to w(
            Tones.INFO, "Cashier's override per day",
            "The most one cashier may accept against a red screen in a day.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "manager_override_ceiling" to w(
            Tones.INFO, "Manager's override ceiling",
            "The most a manager may accept against a red screen, over all their overrides.",
            Who.OWNER, "Whole ${c.plural}. 0 turns it off.",
        ),
        "heartbeat_max_age_secs" to w(
            Tones.INFO, "Heartbeat age for instant",
            "How old this phone's last report may be before counters stop accepting instant sales.",
            Who.OWNER, "Whole minutes, from 1 to 60.",
        ),
        "tor_on" to w(
            Tones.INFO, "Tor on the counters",
            "Whether the counters connect through Tor. Turning it off removes that protection.",
            Who.OWNER, "Choose On or Off.",
        ),
        "customer_wifi" to w(
            Tones.INFO, "Customers can join the Wi-Fi",
            "Whether customers can join the Wi-Fi a counter is on, or you are not sure. No removes a Tor requirement.",
            Who.OWNER, "Choose Yes or unsure, or No.",
        ),
    )

    fun editField(id: String, c: ShopCurrency = ShopCurrency.Usd): SayWords =
        editFieldsIn(c).firstOrNull { it.first == id }?.second ?: throw IllegalArgumentException("no field $id")

    // ---- the till ------------------------------------------------------------------------------

    val posNotConnected = w(
        Tones.INFO, "No till connected",
        "This phone is not reading any till, so no order is matched against the counters' sales.",
        Who.OWNER,
        "Choose the till's vendor, type its location, and paste a read-only sign-in, then connect.",
    )

    val posNeedsLink = w(
        Tones.INFO, "Till needs the link",
        "Matching a till needs the link to the counters, and this phone was set up without a relay.",
        Who.OWNER,
        "Nothing to do here. A setup text with a relay adds the link.",
    )

    val posTokenNotAccepted = w(
        Tones.STOP, "Sign-in not accepted",
        "A read-only sign-in is 1 to 512 letters, digits and signs, with no space in it.",
        Who.OWNER,
        "Paste it again from the till's system, or make a new one there.",
    )

    val posConnected = w(
        Tones.GOOD, "Till connected",
        "The till is connected, and its sign-in is sealed on this phone and is not shown again.",
        Who.NOBODY,
        "Nothing to do. Keep the app open: the till is read about once a minute.",
    )

    val posRemoved = w(
        Tones.INFO, "Till taken off",
        "The till's settings, its sign-in and the matches kept for it are removed from this phone.",
        Who.NOBODY,
        "Nothing to do. Connect a till again when you want its orders matched.",
    )

    val posAcknowledged = w(
        Tones.INFO, "Alert acknowledged",
        "The alert is acknowledged on this phone only: the Box and the counters do not learn of it.",
        Who.NOBODY,
        "Nothing to do.",
    )

    val posTokenMissing = w(
        Tones.STOP, "Sign-in missing",
        "A till is connected, but this phone holds no sign-in for it, so nothing is read.",
        Who.OWNER,
        "Replace the sign-in with a new read-only one from the till's system.",
    )

    val posAddressRefused = w(
        Tones.STOP, "Till address refused",
        "The library asked for an address that is not the till's system, so nothing was sent.",
        Who.OWNER,
        "Close the app and open it again. If it happens again, do not rely on the matching.",
    )

    val posPageTooLong = w(
        Tones.STOP, "Till answer too long",
        "The till's system sent an answer longer than 8 megabytes, so it was not read.",
        Who.OWNER,
        "Nothing to do now. The next read is in about a minute.",
    )

    val posReadCutOff = w(
        Tones.STOP, "Till read cut off",
        "One read asked for more pages than a read may take, so it was ended.",
        Who.OWNER,
        "Nothing to do now. The next read is in about a minute.",
    )

    fun posNotReached(problem: String?) = w(
        Tones.STOP, "Till not reached",
        "The till's system did not answer through Tor, so nothing was read.",
        Who.OWNER,
        "Check that the phone is online and Tor is connected. The next read is in about a minute.",
        detailOf(problem),
    )

    // ---- the node ----------------------------------------------------------------------------

    val starting = w(
        Tones.INFO, "Starting",
        "The owner node is starting and looking for the network.",
        Who.NOBODY,
        "Wait a moment. Keep the app open.",
    )

    val stopped = w(
        Tones.STOP, "Owner node stopped",
        "The owner node has stopped, so the wallet is no longer followed.",
        Who.OWNER,
        "Close the app and open it again.",
    )

    // ---- the app's own faults --------------------------------------------------------------

    fun libraryMissing(problem: String?) = w(
        Tones.STOP, "The app is incomplete",
        "The part of this app that runs the owner node did not load, so nothing was done.",
        Who.OWNER,
        "Do not use this copy. Install the app again from the place you got it.",
        detailOf(problem),
    )

    /** An error the Kotlin side made itself, with no words from the library. */
    fun fault(e: OwnerException) = w(
        Tones.STOP, "Answer not readable",
        "The app could not read an answer from the owner node, so nothing was changed.",
        Who.OWNER,
        "Close the app and open it again. If it happens again, stop and do not spend.",
        "${e.rule}: ${e.text}".take(200),
    )

    fun storageUnavailable(problem: String?) = w(
        Tones.STOP, "Phone storage not usable",
        "The phone's protected storage did not start, so the saved setup was not opened.",
        Who.OWNER,
        "Restart the phone and open this app again. If it happens again, stop and do not spend.",
        detailOf(problem),
    )

    fun fileNotRead(problem: String?) = w(
        Tones.STOP, "File not read",
        "The file could not be opened or read, so nothing was done.",
        Who.OWNER,
        "Pick the file again, or paste the text instead.",
        detailOf(problem),
    )

    fun fileNotSaved(problem: String?) = w(
        Tones.STOP, "Not saved",
        "The app could not write to this phone's storage, so nothing was saved.",
        Who.OWNER,
        "Try again. If it happens again, free some space on the phone.",
        detailOf(problem),
    )

    // ---- the PIN lock (OA-60 to OA-65) ----------------------------------------------------

    /** A new setup is locked behind a PIN before it is saved, so the PIN comes first. */
    val choosePin = w(
        Tones.INFO, "Choose a PIN",
        "Key 1 on this phone is kept behind a PIN, which the app asks for every time it opens.",
        Who.OWNER,
        "Choose at least 6 digits, or a passphrase of at least 8 characters, and do not forget it.",
    )

    /** A setup saved by an older build has no PIN: it is locked behind one and used as it is. */
    val upgradePin = w(
        Tones.INFO, "Set a PIN for key 1",
        "Key 1 on this phone was saved without a PIN, so anyone who opens the app could use it.",
        Who.OWNER,
        "Choose a PIN now; the setup is kept as it is and locked behind it.",
    )

    /** The first screen of every start and every return after more than a minute away. */
    val unlockSetup = w(
        Tones.INFO, "Enter your PIN",
        "Key 1 on this phone is locked, so the app opens only with the PIN or passphrase.",
        Who.OWNER,
        "Type it and tap Unlock. Too many wrong tries wipe key 1 from this phone.",
    )

    val pinsDiffer = w(
        Tones.STOP, "The two PINs differ",
        "The PIN typed the second time is not the same as the first, so nothing was set.",
        Who.OWNER,
        "Type the same PIN in both fields, then try again.",
    )

    val pinChanged = w(
        Tones.GOOD, "PIN changed",
        "Key 1 on this phone is now locked behind the new PIN.",
        Who.NOBODY,
        "Nothing to do. Use the new PIN from now on.",
    )

    /** The page is drawn before the slow step: the key from the PIN takes about a second. */
    val working = w(
        Tones.INFO, "Working",
        "The app is working with the PIN, which takes a moment.",
        Who.NOBODY,
        "Wait. Do not close the app.",
    )

    val waitForTry = w(
        Tones.INFO, "Wait before the next try",
        "There were too many wrong tries, so the app waits before it takes another.",
        Who.OWNER,
        "Wait for the time shown, then type the PIN again.",
    )

    fun triesNotSaved(problem: String?) = w(
        Tones.STOP, "Try not counted",
        "The phone could not keep the count of wrong tries, so the PIN was not tried.",
        Who.OWNER,
        "Try again. If it keeps failing, restart the phone and open this app again.",
        detailOf(problem),
    )

    /** OA-64: the wipe happened, and what to do next: the kit's move to a new wallet, or the holder of key 2. */
    val wiped = w(
        Tones.STOP, "Key 1 wiped from this phone",
        "This phone's copy of key 1 was wiped after 10 wrong tries. The money is not lost: two of the three keys still exist.",
        Who.OWNER,
        "Follow the recovery kit to move to a new wallet, or ask the person who holds key 2.",
    )

    fun wipeFailed(problem: String?) = w(
        Tones.STOP, "Wipe not finished",
        "Key 1 could not be wiped from this phone completely, so the app will try again when it opens.",
        Who.OWNER,
        "Close the app and open it again. If it keeps failing, uninstall the app.",
        detailOf(problem),
    )

    /** SC-12: the list of words as the table of docs/screens/owner-app.md. */
    fun screensMarkdown(): String {
        fun who(n: Int) = when (n) {
            Who.NOBODY -> "nobody"
            Who.CUSTOMER -> "the customer"
            Who.CASHIER -> "the cashier"
            Who.MANAGER -> "a manager"
            Who.OWNER -> "the owner"
            Who.KEY_HOLDER -> "the key holder"
            else -> "the Box keeper"
        }
        fun cell(text: String) = text.replace('|', '/')
        val out = StringBuilder(
            "# Owner app: what the app itself tells a person\n\n" +
                "Made by `AppWords.screensMarkdown` in\n" +
                "`apps/owner-android/src/model/AppWords.kt`. Do not edit it by hand: run\n" +
                "`apps/owner-android/tests/run-jvm.sh` with the environment variable\n" +
                "`OPENBMT_WRITE_SCREENS=1` to write it again.\n\n" +
                "Everything about the coins, the payments, the queue, the link, the\n" +
                "old addresses and the refusals of the rules is shown with the words of\n" +
                "the library (`docs/screens/owner.md`, `docs/screens/owner-node.md`,\n" +
                "`openbmt-owner-ffi`). These are the words of what the app decides itself.\n\n" +
                "| State | Heading | What happened | What to do next | Who | Code |\n" +
                "|---|---|---|---|---|---|\n",
        )
        for ((name, w) in all()) {
            out.append("| ${cell(name)} | ${cell(w.heading)} | ${cell(w.what)} | ${cell(w.next)} | ${who(w.who)} | ${w.code ?: ""} |\n")
        }
        out.append(
            "\nWhat Android shows for the background watch, which is not a screen of the app:\n\n" +
                "- Rest of the setting's text:\n  $watchSettingMore\n" +
                "- Notification title:\n  $watchNotificationTitle\n" +
                "- Notification text:\n  $watchNotificationText\n" +
                "- Notification action:\n  $watchNotificationStop\n" +
                "- Notification when Tor is not connected:\n  $watchNotificationNoOrbot\n",
        )
        out.append(
            "\nThe first run: the welcome, the practice shop, the shop, the till and its\n" +
                "steps, in `FirstRunWords`. `{till}` is the till's name as listed:\n\n" +
                "| Key | Text |\n" +
                "|---|---|\n",
        )
        for ((key, text) in FirstRunWords.all()) out.append("| ${cell(key)} | ${cell(text)} |\n")
        out.append(
            "\nConnect your wallet, after the advice on an own wallet, in `ConnectWords`:\n\n" +
                "| Key | Text |\n" +
                "|---|---|\n",
        )
        for ((key, text) in ConnectWords.all()) out.append("| ${cell(key)} | ${cell(text)} |\n")
        out.append(
            "\nAdding a counter to a shop with its own wallet (OA-105 to OA-114), in\n" +
                "`OwnPairWords`. `{name}` is the counter's name and `{n}` a number of\n" +
                "addresses. A text named `WORDS_NEEDED_OA_...` is not written yet and shows\n" +
                "its own name:\n\n" +
                "| Key | Text |\n" +
                "|---|---|\n",
        )
        for ((key, text) in OwnPairWords.all()) out.append("| ${cell(key)} | ${cell(text)} |\n")
        out.append(
            "\nThe key map at the top of the key-setup screens. `{k1}`, `{k2}` and `{k3}` are\n" +
                "the state words in lower case:\n\n" +
                "| Key | Text |\n" +
                "|---|---|\n",
        )
        for ((key, text) in keyMapAll()) out.append("| ${cell(key)} | ${cell(text)} |\n")
        out.append(
            "\nThe starter wallet (`docs/START-SIMPLE-RULES.md`, SS-10 to SS-43), in\n" +
                "`StarterWords`. A text named `WORDS_NEEDED_OA_SS...` is not written yet\n" +
                "and shows its own name. The ones with `CORE` in the name are the Rust\n" +
                "core's own words, which the binding does not carry yet:\n\n" +
                "| Key | Text |\n" +
                "|---|---|\n",
        )
        for ((key, text) in StarterWords.all()) out.append("| ${cell(key)} | ${cell(text)} |\n")
        out.append(
            "\nThe owner's payment requests (`docs/REQUEST-RULES.md`, RQ-35), in\n" +
                "`RequestWords`. Each text is listed under the rule of its screen; a\n" +
                "message of the app's own is its heading and two sentences, under keys\n" +
                "that end in `_HEADING`, `_WHAT` and `_NEXT`. The library's own words (a\n" +
                "refusal, a row of the list, the state of a request, the line above a\n" +
                "link) are shown as they arrive:\n\n" +
                "| Key | Text |\n" +
                "|---|---|\n",
        )
        for ((key, text) in RequestWords.all()) out.append("| ${cell(key)} | ${cell(text)} |\n")
        out.append(
            "\nThe web store (`docs/WEB-ORDER-RULES.md`, WO-01 to WO-32), in\n" +
                "`WebOrderWords`. What the library says of a refusal or a failed visit\n" +
                "is in `crates/openbmt-owner/src/web_order/say.rs`:\n\n" +
                "| Key | Text |\n" +
                "|---|---|\n",
        )
        for ((key, text) in WebOrderWords.all()) out.append("| ${cell(key)} | ${cell(text)} |\n")
        return out.toString()
    }

    /** Every fixed words of the app, for the test that checks them all and writes the list. */
    fun all(): List<Pair<String, SayWords>> = listOf(
        "notSetUp" to notSetUp,
        "setupSaved" to setupSaved,
        "setupNotReadable" to setupNotReadable("a problem"),
        "torAddressNotAccepted" to torAddressNotAccepted,
        "torAlwaysOn" to torAlwaysOn,
        "orbotNotRunning" to orbotNotRunning,
        "torBuiltIn" to torBuiltIn,
        "torUsesOrbot" to torUsesOrbot,
        "torStarting" to torStarting,
        "torFailed" to torFailed,
        "notANumber" to notANumber,
        "counterNeeded" to counterNeeded,
        "counterNotAccepted" to counterNotAccepted,
        "responseNeeded" to responseNeeded,
        "responseNotReadable" to responseNotReadable("a problem"),
        "requestReady" to requestReady,
        "requestCopied" to requestCopied,
        "requestCodeAnimated" to requestCodeAnimated,
        "requestCodeStill" to requestCodeStill,
        "requestCodeNotMade" to requestCodeNotMade,
        "scanCancelled" to scanCancelled,
        "scanNotAResponse" to scanNotAResponse,
        "counterReading" to counterReading,
        "counterNotACode" to counterNotACode,
        "counterScanCancelled" to counterScanCancelled,
        "counterCodeNotRequest" to counterCodeNotRequest,
        "sharesNote" to sharesNote(2),
        "sharesNoteOne" to sharesNote(1),
        "counterCompare" to counterCompare,
        "counterPriceNeeded" to counterPriceNeeded,
        "counterPriceFetching" to counterPriceFetching,
        "counterPriceFetched" to counterPriceFetched(98_500),
        "priceNotRead" to priceNotRead,
        "counterCancelled" to counterCancelled,
        "counterNotPaired" to counterNotPaired,
        "counterCodeAnimated" to counterCodeAnimated,
        "counterCodeStill" to counterCodeStill,
        "counterCodeNoBatch" to counterCodeNoBatch,
        "counterCodeNotMade" to counterCodeNotMade,
        "counterDelivered" to counterDelivered,
        "ceremonyMakingKey" to ceremonyMakingKey,
        "ceremonyScanNext" to ceremonyScanNext,
        "ceremonyNotAnAnswer" to ceremonyNotAnAnswer,
        "ceremonyCodeTooLong" to ceremonyCodeTooLong,
        "ceremonyScanCancelled" to ceremonyScanCancelled,
        "ceremonyCompareRoot" to ceremonyCompareRoot,
        "ceremonyRootDiffers" to ceremonyRootDiffers,
        "ceremonyKey2Needed" to ceremonyKey2Needed,
        "ceremonyKey2Text" to ceremonyKey2Text,
        "ceremonyKey2ScanCancelled" to ceremonyKey2ScanCancelled,
        "ceremonyBuilding" to ceremonyBuilding,
        "ceremonyCheckWallet" to ceremonyCheckWallet,
        "ceremonyCodeAnimated" to ceremonyCodeAnimated,
        "ceremonyCodeStill" to ceremonyCodeStill,
        "ceremonyCodeNotMade" to ceremonyCodeNotMade,
        "ceremonyCompareSetup" to ceremonyCompareSetup,
        "ceremonyWordsDiffer" to ceremonyWordsDiffer,
        "ceremonyScanAgain" to ceremonyScanAgain,
        "ceremonyFinishing" to ceremonyFinishing,
        "ceremonySaveNext" to ceremonySaveNext,
        "ceremonyResume" to ceremonyResume,
        "ceremonyStartAgain" to ceremonyStartAgain,
        "ceremonyThrownAway" to ceremonyThrownAway,
        "ceremonyRecordNotRead" to ceremonyRecordNotRead("a problem"),
        "setupTextNeeded" to setupTextNeeded,
        "watchSetting" to watchSetting,
        "watchNotificationsOff" to watchNotificationsOff,
        "requestSaved" to requestSaved("request.obm"),
        "refundQueuedAtDepth" to refundQueuedAtDepth,
        "refundQueuedWaiting" to refundQueuedWaiting,
        "cashOutQueued" to cashOutQueued,
        "cashOutPrice" to cashOutPrice(98_500, ShopCurrency.Usd),
        "cashOutPriceNeeded" to cashOutPriceNeeded,
        "booksEmpty" to booksEmpty,
        "booksSaved" to booksSaved,
        "entryCancelled" to entryCancelled,
        "entryRequeued" to entryRequeued,
        "sessionAbandoned" to sessionAbandoned,
        "abandonExported" to abandonExported,
        "abandonedExported" to abandonedExported,
        "alertCleared" to alertCleared,
        "noteDismissed" to noteDismissed,
        "noLink" to noLink,
        "noSettingsYet" to noSettingsYet,
        "editIntro" to editIntro,
        "editGroupCurrency" to editGroupCurrency,
        "editCurrencyRetype" to editCurrencyRetype,
        "editGroupInstant" to editGroupInstant,
        "editGroupPayments" to editGroupPayments,
        "editGroupTips" to editGroupTips,
        "editGroupRefunds" to editGroupRefunds,
        "editGroupSafety" to editGroupSafety,
        "editGroupShares" to editGroupShares,
        "editGroupLightning" to editGroupLightning,
        "editLightningNote" to editLightningNote,
        "editNothingChanged" to editNothingChanged,
        "editNeedsKeyApp" to editNeedsKeyApp,
        "editTakesEffect" to editTakesEffect,
        "editQueuedLoosening" to editQueuedLoosening,
        "editQueuedTightening" to editQueuedTightening,
        "editNodeNeeded" to editNodeNeeded,
        "editNoCounters" to editNoCounters,
        "editSharesNotPercent" to editSharesNotPercent,
        "editTipNotRead" to editTipNotRead,
        "editValueNotReadDollars" to editValueNotRead(FieldKind.Dollars, "Refunds per day"),
        "editValueNotReadPercent" to editValueNotRead(FieldKind.Percent, "Underpayment still counted"),
        "editValueNotReadBlocks" to editValueNotRead(FieldKind.Blocks, "Blocks to settle a sale"),
        "editValueNotReadMinutes" to editValueNotRead(FieldKind.Minutes, "Heartbeat age for instant"),
        "editValueNotReadSats" to editValueNotRead(FieldKind.Sats, "Smallest bitcoin sale"),
        "editValueNotReadSwitch" to editValueNotRead(FieldKind.Switch, "Tor on the counters"),
        "posNotConnected" to posNotConnected,
        "posNeedsLink" to posNeedsLink,
        "posTokenNotAccepted" to posTokenNotAccepted,
        "posConnected" to posConnected,
        "posRemoved" to posRemoved,
        "posAcknowledged" to posAcknowledged,
        "posTokenMissing" to posTokenMissing,
        "posAddressRefused" to posAddressRefused,
        "posPageTooLong" to posPageTooLong,
        "posReadCutOff" to posReadCutOff,
        "posNotReached" to posNotReached("a problem"),
        "starting" to starting,
        "stopped" to stopped,
        "libraryMissing" to libraryMissing("no library"),
        "fault" to fault(OwnerException("response-null", "the library gave no response", 4)),
        "storageUnavailable" to storageUnavailable("a problem"),
        "fileNotRead" to fileNotRead("a problem"),
        "fileNotSaved" to fileNotSaved("a problem"),
        "choosePin" to choosePin,
        "upgradePin" to upgradePin,
        "unlockSetup" to unlockSetup,
        "pinsDiffer" to pinsDiffer,
        "pinChanged" to pinChanged,
        "working" to working,
        "waitForTry" to waitForTry,
        "triesNotSaved" to triesNotSaved("a problem"),
        "wiped" to wiped,
        "wipeFailed" to wipeFailed("a problem"),
        "cameraOff" to CameraWords.off(Who.OWNER),
        "cameraNotStarted" to CameraWords.notStarted(Who.OWNER, "a problem"),
    ) + editFields.map { (id, words) -> "editField_$id" to words } + BoxWords.all() + UrWords.forReading(Who.OWNER) +
        HardwareWords.all() + OwnSpendWords.all()
}
