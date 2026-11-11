package org.openbmt.counter.app

import org.openbmt.app.SayWords

/** Where the pairing ceremony of this screen has got to (CR-98). */
enum class PairPhase {
    /** No ceremony is under way. */
    Idle,

    /** The camera is open for the Key app's offer. */
    ScanOffer,

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

    /** The person tapped to pair: the camera opens for the offer. */
    fun start(): Boolean = move(PairPhase.Idle, PairPhase.ScanOffer)

    /** The offer was read and the library gave a request and four words. */
    fun offerRead(): Boolean = move(PairPhase.ScanOffer, PairPhase.Compare)

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
