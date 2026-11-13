package org.openbmt.counter.app

import org.openbmt.app.SayWords
import org.openbmt.counter.CounterException
import org.openbmt.counter.SwitchSame
import org.openbmt.counter.Words
import java.io.IOException

/**
 * The page where a manager checks the new wallet's first address when the owner moves the shop off the starter wallet
 * (docs/COUNTER-RULES.md CR-259 to CR-262), modelled on [ConfirmChange]. "Same address" asks for the manager's PIN
 * first (CR-261): a PIN that is wrong or held changes nothing. With a right PIN the library writes its first step and
 * gives the new pairing record; the app seals and keeps that record, and only then tells the library it was kept. A
 * stop between the two is finished at the next start from the record the library still holds ([finish]).
 */
object ConfirmSwitch {
    /** The new wallet's first address as the largest thing on the page: groups of four characters, as the pairing shows it (CR-228). */
    fun address(address: String): String = FirstAddress.groups(address)

    /** What a tap on "Same address" or "Different" comes to. */
    sealed class Outcome {
        /** The record was kept and the counter now sells to the new wallet: the library's words say so. */
        class Switched(val words: SayWords) : Outcome()

        /** "Different": the switch was dropped and the refusal written for the owner. */
        class Dropped(val words: SayWords) : Outcome()

        /** The PIN was wrong or held: nothing changed. */
        class PinRefused(val words: SayWords) : Outcome()

        /** The library refused. */
        class Refused(val words: SayWords) : Outcome()

        /** The new record could not be kept: the library finishes at the next start, or at the next look at home. */
        class NotKept(val words: SayWords) : Outcome()
    }

    /**
     * The PIN is checked first. Only a right PIN reaches [switchSame]; a wrong one or a held one changes nothing. The
     * count of wrong tries is the PIN store's own (CR-85). [keep] seals and saves the record the library gave; [kept]
     * tells the library it was done.
     */
    fun same(pin: String, pins: PinStore, switchSame: () -> SwitchSame, keep: (ByteArray) -> Unit, kept: () -> Words): Outcome {
        when (val r = pins.check(pin)) {
            PinResult.Ok -> Unit
            is PinResult.Wrong -> return Outcome.PinRefused(AppWords.pinWrong(r.waitSecs))
            is PinResult.Locked -> return Outcome.PinRefused(AppWords.pinLocked(r.waitSecs))
        }
        return try {
            keepThen(switchSame().record, keep, kept)
        } catch (e: CounterException) {
            Outcome.Refused(e.sayWords())
        }
    }

    /**
     * CR-261: the record the library still holds after a start that found a switch half done, or null when it holds
     * none. Keeps it and says so; the outcome is null when nothing waited.
     */
    fun finish(update: ByteArray?, keep: (ByteArray) -> Unit, kept: () -> Words): Outcome? {
        if (update == null) return null
        return try {
            keepThen(update, keep, kept)
        } catch (e: CounterException) {
            Outcome.Refused(e.sayWords())
        }
    }

    /** "Different" (CR-260): needs no PIN, since it only keeps the counter on the wallet it already pays. */
    fun different(drop: () -> Words): Outcome =
        try {
            Outcome.Dropped(drop().say())
        } catch (e: CounterException) {
            Outcome.Refused(e.sayWords())
        }

    /** Keep the record, then tell the library and show its words. The outcome is [Outcome.NotKept] when the file could not be written, and then the library is not told. */
    private fun keepThen(record: ByteArray, keep: (ByteArray) -> Unit, kept: () -> Words): Outcome {
        try {
            keep(record)
        } catch (e: IOException) {
            return Outcome.NotKept(AppWords.fileNotSaved(e.message))
        }
        return Outcome.Switched(kept().say())
    }
}
