package org.openbmt.counter.app

import org.openbmt.app.SayWords
import org.openbmt.counter.ChangeField
import org.openbmt.counter.ChangeValue
import org.openbmt.counter.ChangeValueKind
import org.openbmt.counter.PendingChange
import org.openbmt.counter.Words

/**
 * The page where a manager confirms a loosening of the shop's settings that came from the owner phone
 * (docs/COUNTER-RULES.md CR-239 to CR-244): the labels of the fields, the values as text, the rows of the list, and
 * the one step that matters: the manager's PIN is checked first, and a PIN that is wrong or held signs nothing.
 */
object ConfirmChange {
    /** The label of a field: the one the settings editor of the owner app uses, or the word that says there is none yet. */
    fun label(field: String): String = when (field) {
        "light_per_sale" -> "Light check: one sale"
        "light_budget" -> "Light check: all at once"
        "node_per_sale" -> "Own node: one sale"
        "node_budget" -> "Own node: all at once"
        "box_per_sale" -> "Box only: one sale"
        "box_budget" -> "Box only: all at once"
        "underpayment_tolerance_bps" -> "Underpayment still counted"
        "min_confirmations" -> "Blocks to settle a sale"
        "final_confirmations" -> "Blocks until final"
        "min_sale_sats" -> "Smallest bitcoin sale"
        "refund_cap" -> "Largest single refund"
        "refund_daily_cap" -> "Refunds per day"
        "cashier_override_daily_cap" -> "Cashier's override per day"
        "manager_override_ceiling" -> "Manager's override ceiling"
        "heartbeat_max_age_secs" -> "Heartbeat age for instant"
        "tor_on" -> "Tor on the counters"
        "customer_wifi" -> "Customers can join the Wi-Fi"
        else -> AppWords.WORDS_NEEDED_CR_240_FIELD
    }

    /** A value as the owner app shows it: a flag is On or Off (Tor) and Yes or unsure or No (the Wi-Fi), a percentage has its percent sign. */
    fun value(field: String, v: ChangeValue): String = when (v.kind) {
        ChangeValueKind.Units -> if (v.number == 0L) "Off" else v.number.toString()
        ChangeValueKind.Sats -> "${v.number} sats"
        ChangeValueKind.Bps -> "${v.number / 100}.${(v.number % 100).toString().padStart(2, '0')}%"
        ChangeValueKind.Count -> v.number.toString()
        ChangeValueKind.Seconds -> "${v.number} seconds"
        ChangeValueKind.Flag -> when {
            field == "customer_wifi" -> if (v.number != 0L) "Yes or unsure" else "No"
            else -> if (v.number != 0L) "On" else "Off"
        }
        ChangeValueKind.Code -> v.text
    }

    /** One row of the list: the field, then the value now and the value after. */
    class Row(val field: String, val change: String)

    fun rows(p: PendingChange): List<Row> = p.fields.map { f -> Row(label(f.field), "${value(f.field, f.now)} → ${value(f.field, f.after)}") }

    /** The second line of the manager's menu row. */
    fun menuSub(p: PendingChange): String = "From the owner phone, version ${p.version}"

    /** What a tap on the confirm button comes to. */
    sealed class Outcome {
        /** The PIN was right and the counter signed: its words say so. */
        class Confirmed(val words: SayWords) : Outcome()

        /** The PIN was wrong or held: nothing was signed. */
        class PinRefused(val words: SayWords) : Outcome()

        /** The library refused or the file could not be read. */
        class Refused(val words: SayWords) : Outcome()
    }

    /**
     * The PIN is checked first. Only a right PIN reaches [sign]; a wrong one or a held one signs nothing and the
     * counter's record is not written. The count of wrong tries is the PIN store's own (CR-85).
     */
    fun confirm(pin: String, pins: PinStore, sign: () -> Words): Outcome {
        when (val r = pins.check(pin)) {
            PinResult.Ok -> Unit
            is PinResult.Wrong -> return Outcome.PinRefused(AppWords.pinWrong(r.waitSecs))
            is PinResult.Locked -> return Outcome.PinRefused(AppWords.pinLocked(r.waitSecs))
        }
        return try {
            Outcome.Confirmed(sign().say())
        } catch (e: org.openbmt.counter.CounterException) {
            Outcome.Refused(e.sayWords())
        }
    }
}
