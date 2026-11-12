package org.openbmt.counter.app

import org.openbmt.app.SayWords
import org.openbmt.counter.OwnWalletKind

/** Where the pairing ceremony of this screen has got to (CR-98). */
enum class PairPhase {
    /** No ceremony is under way. */
    Idle,

    /** The camera is open for the offer: the Key app's, or the owner phone's for the shop's own wallet. */
    ScanOffer,

    /** The offer was the owner phone's: the wallet's first address is on the screen, waiting for "Same address" (CR-228). */
    CheckAddress,

    /** The request and its four words are on the screen, waiting for the person to compare them. */
    Compare,

    /** The person said "They match"; the camera is open for the owner phone's bundle. */
    ScanBundle,
}

/**
 * The steps of the ceremony as the screens take them, apart from the camera and the library. The
 * library holds the ceremony itself (CR-91); this keeps the order of the screens and says when the
 * library must be told to forget: the camera for the bundle opens only after "They match", and a
 * ceremony that was open when the app was left is forgotten, with a message when the app is back.
 * Every move that is not allowed from the phase it is made in is refused with `false` and changes
 * nothing.
 */
class PairingProgress {
    var phase: PairPhase = PairPhase.Idle
        private set

    private var forgotten = false

    /** Whether the ceremony of the last [start] was one with the shop's own wallet (CR-237): which words say it was forgotten. */
    var own: Boolean = false
        private set

    /** The person tapped to pair: the camera opens for the offer. */
    fun start(): Boolean = move(PairPhase.Idle, PairPhase.ScanOffer).also { if (it) own = false }

    /** The Key app's offer was read and the library gave a request and four words. */
    fun offerRead(): Boolean = move(PairPhase.ScanOffer, PairPhase.Compare)

    /** The owner phone's offer was read and the library gave the wallet's first address: no request yet (CR-228). */
    fun offerReadOwn(): Boolean = move(PairPhase.ScanOffer, PairPhase.CheckAddress).also { if (it) own = true }

    /** "Same address": the library made the request and the four words (CR-229). */
    fun addressSame(): Boolean = move(PairPhase.CheckAddress, PairPhase.Compare)

    /** "Different": the ceremony is over before any key was made (CR-228). */
    fun addressDifferent(): Boolean = move(PairPhase.CheckAddress, PairPhase.Idle)

    /** "They match": the only way to the camera for the bundle. */
    fun matched(): Boolean = move(PairPhase.Compare, PairPhase.ScanBundle)

    /** "They differ": the ceremony is over. */
    fun differed(): Boolean = move(PairPhase.Compare, PairPhase.Idle)

    /** The record was made and saved, or the bundle was refused: the ceremony is over. */
    fun finished(): Boolean = move(PairPhase.ScanBundle, PairPhase.Idle)

    /** The person cancelled or a step failed: the ceremony is over. Returns whether one was open. */
    fun cancelled(): Boolean {
        val open = phase != PairPhase.Idle
        phase = PairPhase.Idle
        return open
    }

    /**
     * The app was left (or closed) while a ceremony was open: it is over, and [takeForgotten] will
     * say so once. Returns whether one was open, so that the caller tells the library to forget it.
     */
    fun left(): Boolean {
        val open = cancelled()
        if (open) forgotten = true
        return open
    }

    /** Whether a ceremony was forgotten because the app was left; true once. */
    fun takeForgotten(): Boolean = forgotten.also { forgotten = false }

    private fun move(from: PairPhase, to: PairPhase): Boolean {
        if (phase != from) return false
        phase = to
        return true
    }
}

/** The first-address screen of a pairing with the shop's own wallet (CR-228, CR-237). */
object FirstAddress {
    /** The address in groups of four characters, as the largest thing on the screen, for the person to compare. */
    fun groups(address: String): String = address.chunked(4).joinToString(" ")

    /** The line beneath the address that says what kind of wallet it is. */
    fun kindLine(kind: OwnWalletKind): String = when (kind) {
        OwnWalletKind.SingleKey -> "Wallet with one key"
        OwnWalletKind.TwoOfThree -> "Wallet with 2 of 3 keys"
        OwnWalletKind.None -> ""
    }
}

/** What is checked on the first screen before the camera opens for the offer. */
/** The Network screen's check (CR-87): the address of Orbot is a host, a colon and a port. */
object NetworkCheck {
    fun problem(address: String): SayWords? = if (HostPort.valid(address)) null else AppWords.torAddressNotAccepted
}

object FirstStartCheck {
    /**
     * The words of the first thing wrong with what was typed, or null when the counter's name, the
     * manager's name and the PIN (twice) are all fine. The counter's name is given to the library,
     * which checks it again by its own rule.
     */
    fun problem(counterName: String, managerName: String, pin1: String, pin2: String): SayWords? = when {
        !validCounterName(counterName) -> AppWords.counterNameNotAccepted
        !AppConfig.validName(managerName) -> AppWords.nameNotAccepted
        !PinStore.validPin(pin1) || pin1 != pin2 -> AppWords.pinNotAccepted
        else -> null
    }

    /** The counter's name as the pairing takes it: 1 to 32 characters, at most 128 bytes, no control character. */
    fun validCounterName(name: String): Boolean =
        name.codePointCount(0, name.length) in 1..32 && name.toByteArray(Charsets.UTF_8).size <= 128 &&
            name.none { Character.isISOControl(it) }

    /** The four words as one line to read and compare: spaced so that each stands apart. */
    fun wordsLine(words: List<String>): String = words.joinToString("   ")
}
