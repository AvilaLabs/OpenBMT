package org.openbmt.key.app

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import org.openbmt.app.KeyTextOutcome
import org.openbmt.app.SayWords
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.key.Key3View
import org.openbmt.key.KeyException
import org.openbmt.key.SlotView
import org.openbmt.key.WalletView
import org.openbmt.key.Words

/**
 * What reading a key 3 export gave: its text for the library, the words of why not (back to the key 3
 * screen), or the words of a code that was read but is not taken as a key (the scan goes on from its page).
 */
sealed class Key3Input {
    class Text(val text: String) : Key3Input()

    class Refused(val words: Words) : Key3Input()

    class Declined(val words: Words) : Key3Input()
}

/** What the screen after key 3 is taken shows, as text. */
class Key3Check(
    /** The master fingerprint in groups, as the person compares it with the other wallet. */
    val fingerprint: String,
    val network: String,
    /** The lines under the Advanced fold. */
    val advanced: List<String>,
)

/** What the comparison screen shows, as text. */
class WalletCheck(
    /** The four checksum words on one line. */
    val checkWords: String,
    /** One line for each of the three keys: its number and its fingerprint in groups. */
    val fingerprints: List<String>,
    /** The identities, as words, under the large text. */
    val identities: List<String>,
    val firstAddress: String,
)

/**
 * The pure parts of the key ceremony screens (DESIGN section 2.1, steps 2 and 5): reading a key 3
 * export, the text the screens show, the shape of the answer and of the wallet setup, and which
 * refusal gets which extra words. No Android class is used, so the JVM test runs it.
 */
object CeremonyModel {
    /** The rule strings of the library's SR-67 refusals (docs/SIGNER-RULES.md). */
    const val RULE_KEY3_MISSING = "key3-missing"
    const val RULE_KEY3_DIFFERS = "key3-differs"
    const val RULE_KEY3_NOT_IN_WALLET = "key3-not-in-wallet"

    /** The magic and the version byte of the Key app's answer (`OBMv`) and of the wallet setup (`OBMw`). */
    private fun frame(last: Char) = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), last.code.toByte(), 1)

    private val ANSWER = frame('v')
    private val SETUP = frame('w')

    private fun hasFrame(bytes: ByteArray, frame: ByteArray) =
        bytes.size > frame.size && frame.indices.all { bytes[it] == frame[it] }

    /** Whether [bytes] begin with the frame of the Key app's answer: only these are drawn as a code. */
    fun looksLikeAnswer(bytes: ByteArray): Boolean = hasFrame(bytes, ANSWER)

    /** Whether [bytes] begin with the frame of the owner phone's wallet setup. */
    fun looksLikeSetup(bytes: ByteArray): Boolean = hasFrame(bytes, SETUP)

    /** The types of UR the key 3 scan takes besides a plain text code (SR-64). */
    val KEY3_TYPES: Set<String> = setOf("crypto-account", "crypto-hdkey", "bytes")

    /**
     * Key 3 from what the library made of a finished UR (UR-31): its text goes on as scanned text does, a
     * code that is not taken as a key shows the library's words and stays on the scan step, and a request
     * or answer outside the framing shows the words of a reader that failed.
     */
    fun readKey3Message(outcome: KeyTextOutcome): Key3Input = when (outcome) {
        is KeyTextOutcome.Key -> Key3Input.Text(outcome.text)
        is KeyTextOutcome.Declined -> Key3Input.Declined(sayToWords(UrWords.keyDeclined(Who.KEY_HOLDER, outcome)))
        is KeyTextOutcome.Refused, KeyTextOutcome.Fault -> Key3Input.Refused(sayToWords(UrWords.readerFailed(Who.KEY_HOLDER)))
    }

    private fun sayToWords(say: SayWords): Words =
        Words(say.tone, say.who, say.heading, say.what, say.next, say.code, null, null, say.detail)

    /** Key 3 from the bytes of a scanned code: plain text goes on, a `ur:` code and bytes that are not text do not. */
    fun readKey3Code(bytes: ByteArray): Key3Input {
        val text = try {
            Charsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes))
                .toString()
        } catch (_: CharacterCodingException) {
            return Key3Input.Refused(AppWords.key3NotText)
        }
        return readKey3Text(text)
    }

    /** Key 3 from pasted or scanned text. Everything else about the text is the library's to refuse. */
    fun readKey3Text(text: String): Key3Input =
        if (text.trimStart().startsWith("ur:", ignoreCase = true)) {
            Key3Input.Refused(AppWords.key3UrNotRead)
        } else {
            Key3Input.Text(text)
        }

    /** A fingerprint of 8 hex digits as two groups of four in capitals; anything else is shown as it is. */
    fun groupFingerprint(fingerprint: String): String =
        if (fingerprint.length == 8) "${fingerprint.substring(0, 4)} ${fingerprint.substring(4)}".uppercase() else fingerprint

    fun key3Check(v: Key3View): Key3Check = Key3Check(
        fingerprint = groupFingerprint(v.masterFingerprint),
        network = if (v.mainnet) "Main bitcoin network" else "A test network",
        advanced = listOf("Origin path: ${v.originPath}", "Account key: ${v.accountXpub}"),
    )

    private fun slotLine(s: SlotView) = "Key ${s.index + 1}: ${groupFingerprint(s.fingerprint)}"

    fun walletCheck(v: WalletView): WalletCheck = WalletCheck(
        checkWords = v.checksumWords.joinToString("   "),
        fingerprints = v.slots.map { slotLine(it) },
        identities = listOf(
            "This app's identity: ${v.ownRootWords.joinToString(" ")}",
            "The owner phone's identity: ${v.ownerRootWords.joinToString(" ")}",
        ),
        firstAddress = v.firstReceiveAddress,
    )

    /**
     * What a refused wallet setup shows: the library's own words, and before them the app's plain words when
     * the owner phone's wallet does not hold the key 3 taken here (SR-67). The library tells the kinds apart by
     * their rule strings; the two that mean "this wallet is not for the key 3 taken" read the same to the person:
     * nothing was accepted and the owner phone's wallet is not this one.
     */
    fun setupRefusal(e: KeyException): List<Words> {
        val own = e.words
        val swapped = e.rule == RULE_KEY3_NOT_IN_WALLET || e.rule == RULE_KEY3_DIFFERS
        return when {
            swapped && own != null -> listOf(AppWords.walletLacksKey3, own)
            swapped -> listOf(AppWords.walletLacksKey3)
            own != null -> listOf(own)
            else -> listOf(AppWords.fault(e))
        }
    }
}
