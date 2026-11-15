package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import org.openbmt.app.DurableFile
import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerStarter
import org.openbmt.owner.StarterChangeKind
import org.openbmt.owner.StarterLimit
import org.openbmt.owner.StarterLimitKind
import org.openbmt.owner.StarterPaid
import org.openbmt.owner.StarterWalletView
import org.openbmt.owner.Words

// The starter wallet's rules that are the app's own and need no Android class (`docs/START-SIMPLE-RULES.md`): what
// the phone is checked for (SS-33, SS-34), the limit and the date the owner sets (SS-35), what the home line and
// the upgrade screen show (SS-36, SS-37), the typed sentence of "Not yet", the keypad (SS-11), what is kept on the
// phone and what its state means (SS-20, SS-23, SS-43) and the lines of a payment (SS-31). The library's own
// decisions (the limit's state, what waits, how long "Not yet" lasts) are asked of a [StarterCore].

/** One enabled accessibility service: its name as the system lists it, and whether it is marked as an accessibility tool. */
class AccessibilityEntry(val name: String, val isTool: Boolean)

/** What the phone says of itself, gathered by `StarterEnvCheck.kt` (SS-33, SS-34). */
class StarterEnvironment(
    val usbDebugging: Boolean,
    val suPaths: List<String>,
    val testKeys: Boolean,
    /** Apps found that may draw over other apps; best effort, the system does not list them all. */
    val overlayApps: List<String>,
    val services: List<AccessibilityEntry>,
    /** `Build.VERSION.SECURITY_PATCH`, "yyyy-mm-dd", or null when the phone gives none. */
    val securityPatch: String?,
    val screenLock: Boolean,
)

enum class FindingKind { UsbDebugging, Root, Overlay, ServiceTool, ServiceNotTool, OldPatch, PatchUnknown }

/** One thing found: [name] is the app or service where there is one; [refuses] when it stops the step. */
class Finding(val kind: FindingKind, val name: String?, val refuses: Boolean)

/** Where a check is made: before the key (and its words), before the owner types the words, before a payment. */
enum class CheckStage { Making, Words, Paying }

enum class CheckVerdict { NoScreenLock, Refused, Confirm, Clear }

class CheckResult(val verdict: CheckVerdict, val findings: List<Finding>)

object StarterChecks {
    /** Where a rooted phone usually has its `su` binary. */
    val SU_PATHS = listOf(
        "/system/bin/su", "/system/xbin/su", "/sbin/su", "/system/sbin/su", "/vendor/bin/su",
        "/system/su", "/su/bin/su", "/system/bin/.ext/.su", "/system/usr/we-need-root/su", "/data/local/xbin/su",
        "/data/local/bin/su", "/data/local/su",
    )

    /** SS-34: the security patch is more than six months old. A missing or unreadable date counts as unknown, not old. */
    fun patchIsOld(patch: String?, today: LocalDate): Boolean {
        val date = parseDate(patch) ?: return false
        return date.plusMonths(6).isBefore(today)
    }

    fun parseDate(text: String?): LocalDate? = try {
        if (text == null) null else LocalDate.parse(text)
    } catch (_: java.time.format.DateTimeParseException) {
        null
    }

    /**
     * SS-33, SS-34: what is found and what it means. No screen lock stops everything (SS-24). An enabled service that
     * is not marked as an accessibility tool refuses the making of the key and the typing or showing of the words; at a
     * payment it is named like the rest. On a phone that does not say (before Android 12) a service is not a tool. The
     * patch is only looked at before the key is made.
     */
    fun decide(env: StarterEnvironment, stage: CheckStage, today: LocalDate): CheckResult {
        if (!env.screenLock) return CheckResult(CheckVerdict.NoScreenLock, emptyList())
        val found = ArrayList<Finding>()
        if (env.usbDebugging) found.add(Finding(FindingKind.UsbDebugging, null, false))
        if (env.suPaths.isNotEmpty() || env.testKeys) found.add(Finding(FindingKind.Root, env.suPaths.firstOrNull(), false))
        for (app in env.overlayApps) found.add(Finding(FindingKind.Overlay, app, false))
        val strict = stage != CheckStage.Paying
        for (s in env.services) {
            found.add(
                if (s.isTool) Finding(FindingKind.ServiceTool, s.name, false)
                else Finding(FindingKind.ServiceNotTool, s.name, strict),
            )
        }
        if (stage == CheckStage.Making) {
            if (env.securityPatch == null || parseDate(env.securityPatch) == null) found.add(Finding(FindingKind.PatchUnknown, null, false))
            else if (patchIsOld(env.securityPatch, today)) found.add(Finding(FindingKind.OldPatch, env.securityPatch, false))
        }
        val verdict = when {
            found.any { it.refuses } -> CheckVerdict.Refused
            found.isNotEmpty() -> CheckVerdict.Confirm
            else -> CheckVerdict.Clear
        }
        return CheckResult(verdict, found)
    }

    /** Whether the step goes on: nothing found, or every finding named and confirmed, and nothing refuses. */
    fun mayGoOn(result: CheckResult, confirmed: Boolean): Boolean = when (result.verdict) {
        CheckVerdict.Clear -> true
        CheckVerdict.Confirm -> confirmed
        CheckVerdict.Refused, CheckVerdict.NoScreenLock -> false
    }

    /** The name and the explanation of a finding, as placeholders until the words are written. */
    fun name(kind: FindingKind): String = when (kind) {
        FindingKind.UsbDebugging -> StarterWords.usbName
        FindingKind.Root -> StarterWords.rootName
        FindingKind.Overlay -> StarterWords.overlayName
        FindingKind.ServiceTool -> StarterWords.serviceName
        FindingKind.ServiceNotTool -> StarterWords.serviceNotToolName
        FindingKind.OldPatch -> StarterWords.patchName
        FindingKind.PatchUnknown -> StarterWords.patchUnknownName
    }

    fun why(kind: FindingKind): String = when (kind) {
        FindingKind.UsbDebugging -> StarterWords.usbWhy
        FindingKind.Root -> StarterWords.rootWhy
        FindingKind.Overlay -> StarterWords.overlayWhy
        FindingKind.ServiceTool -> StarterWords.serviceWhy
        FindingKind.ServiceNotTool -> StarterWords.serviceNotToolWhy
        FindingKind.OldPatch -> StarterWords.patchWhy
        FindingKind.PatchUnknown -> StarterWords.patchUnknownWhy
    }
}

/**
 * What the owner sets (SS-35) and the phone keeps: both in satoshis and seconds, the limit and the date, the switch of
 * SS-38 and "Not yet"; and the times of the nudge card (SS-36) and whether the second copy of the words was made (SS-10).
 */
class StarterRecord(
    val limitSats: Long,
    val moveBy: Long,
    val stopAboveLimit: Boolean,
    val notYetUntil: Long?,
    /** The next change address the starter wallet's payments use (the app counts them; the library signs for the one it is given). */
    val changeIndex: Long = 0,
    /** SS-36: when the nudge card was last shown, in seconds; null when it never was. */
    val cardLastShown: Long? = null,
    /** SS-36: the end of the "Not now" of the card, in seconds; null when none holds. */
    val cardNotNowUntil: Long? = null,
    /** SS-10: whether the words were shown a second time, for a second card, before the key was sealed. */
    val secondCopyMade: Boolean = false,
    /** When the record was made, in seconds; 0 for a record from before this was kept. */
    val made: Long = 0,
    /** The highest wall-clock time the phone has seen while the record was kept, in seconds; a record from before this reads as [made]. */
    val seenMax: Long = made,
) {
    fun toText(): String = "limit-sats: $limitSats\nmove-by: $moveBy\nstop-above-limit: $stopAboveLimit\n" +
        (if (notYetUntil != null) "not-yet-until: $notYetUntil\n" else "") + "change-index: $changeIndex\n" +
        (if (cardLastShown != null) "card-last-shown: $cardLastShown\n" else "") +
        (if (cardNotNowUntil != null) "card-not-now-until: $cardNotNowUntil\n" else "") +
        (if (secondCopyMade) "second-copy-made: true\n" else "") +
        (if (made > 0) "made: $made\n" else "") + (if (seenMax > 0) "seen-max: $seenMax\n" else "")

    private fun copy(
        notYet: Long? = notYetUntil,
        change: Long = changeIndex,
        stop: Boolean = stopAboveLimit,
        shown: Long? = cardLastShown,
        notNow: Long? = cardNotNowUntil,
        seen: Long = seenMax,
    ) = StarterRecord(limitSats, moveBy, stop, notYet, change, shown, notNow, secondCopyMade, made, seen)

    fun withNotYet(until: Long?) = copy(notYet = until)

    fun withNextChange() = copy(change = changeIndex + 1)

    /** SS-38: the record with the stop switched on or off; the rest is kept. */
    fun withStop(stop: Boolean) = copy(stop = stop)

    /** SS-36: the record with the card shown at [now]. */
    fun withCardShown(now: Long) = copy(shown = now)

    /** SS-36: the record with the card put off until [until]. */
    fun withCardNotNow(until: Long?) = copy(notNow = until)

    /** The record with the highest time seen raised to [now]; the same record when [now] is not higher. */
    fun withSeen(now: Long) = if (now > seenMax) copy(seen = now) else this

    /**
     * The time every judgment of the date, the card and "Not yet" is made at: the wall clock, or the highest time the phone
     * has seen when the clock was moved back, so that moving it back does not move the date.
     */
    fun judged(now: Long): Long = maxOf(now, seenMax)

    companion object {
        fun parse(text: String): StarterRecord {
            val map = HashMap<String, String>()
            for (line in text.lines()) {
                if (line.isBlank()) continue
                val i = line.indexOf(": ")
                if (i <= 0) throw IOException("a line of the starter record does not read")
                map[line.substring(0, i)] = line.substring(i + 2)
            }
            fun num(key: String): Long = map[key]?.toLongOrNull()?.takeIf { it >= 0 } ?: throw IOException("the starter record has no $key")
            val limit = num("limit-sats")
            if (limit <= 0) throw IOException("the starter record has no limit")
            val moveBy = num("move-by")
            val made = map["made"]?.toLongOrNull()?.takeIf { it > 0 } ?: 0L
            // The date is at most the 90 days that setup allows from the day the record was made; a record from before that was kept has no check.
            if (made > 0 && moveBy > made + StarterSetup.MAX_DAYS * 86_400L) throw IOException("the starter record's date is more than 90 days after it was made")
            val stop = map["stop-above-limit"]
            if (stop != "true" && stop != "false") throw IOException("the starter record has no stop-above-limit")
            return StarterRecord(
                limit, moveBy, stop == "true", map["not-yet-until"]?.toLongOrNull(),
                map["change-index"]?.toLongOrNull()?.takeIf { it >= 0 } ?: 0,
                map["card-last-shown"]?.toLongOrNull()?.takeIf { it >= 0 }, map["card-not-now-until"]?.toLongOrNull()?.takeIf { it >= 0 },
                map["second-copy-made"] == "true",
                made, map["seen-max"]?.toLongOrNull()?.takeIf { it > 0 } ?: made,
            )
        }
    }
}

/**
 * What the starter line says of the limit and the stop (LK-79, SS-32, SS-38). A counter learns of the stop only through
 * the Box, so the option is shown, and the stop is passed to the node, only for a shop with a Box link.
 */
object StarterLineSettings {
    /** BX-18: whether the shop has a Box added and linked: the link of its own wallet is kept. A file that does not read says no. */
    fun boxLinked(store: OwnShopStore): Boolean = try {
        store.loadLink() != null
    } catch (_: IOException) {
        false
    }

    /** SS-38: whether the starter page offers the stop option. */
    fun stopOptionShown(hasBoxLink: Boolean): Boolean = hasBoxLink

    /** The stop the node is given: the owner's choice, and only while a Box link carries it to the counters. */
    fun stopForNode(record: StarterRecord, hasBoxLink: Boolean): Boolean = record.stopAboveLimit && hasBoxLink
}

/** What the owner types at setup (SS-35): the limit, the date, and the die rolls that SS-03 allows. */
object StarterSetup {
    const val SUGGESTED_DAYS = 30L

    /** SS-35: the furthest the date may be from today, in days. */
    const val MAX_DAYS = 90L
    private const val DAY = 86_400L

    /** The date the app suggests, 30 days on from [now] (seconds); the app never suggests an amount. */
    fun suggestedDate(now: Long): String = LocalDate.ofEpochDay(now / DAY + SUGGESTED_DAYS).toString()

    /** The date typed as yyyy-mm-dd, as the second the day begins (UTC); null when it does not read or is not after [now]. */
    fun moveBy(text: String, now: Long): Long? {
        val date = StarterChecks.parseDate(text.trim()) ?: return null
        val secs = date.atStartOfDay().toEpochSecond(ZoneOffset.UTC)
        return if (secs > now) secs else null
    }

    /** SS-35: whether [moveBy] (seconds) falls on a day more than 90 days after the day [now] (seconds) is in. */
    fun tooLate(moveBy: Long, now: Long): Boolean = moveBy / DAY - now / DAY > MAX_DAYS

    /**
     * The limit typed in whole units of the shop's currency, in satoshis at [pricePerBtc] (whole units of the currency
     * for one bitcoin); null when it is not a whole number above zero or the price is. In Bitcoin Standard (null price)
     * the amount is already satoshis.
     */
    fun limitSats(text: String, pricePerBtc: Long?): Long? {
        val whole = text.trim().replace(",", "").toLongOrNull()?.takeIf { it > 0 } ?: return null
        if (pricePerBtc == null) return whole
        if (pricePerBtc <= 0) return null
        val product = java.math.BigInteger.valueOf(whole).multiply(java.math.BigInteger.valueOf(100_000_000L))
        val sats = product.divide(java.math.BigInteger.valueOf(pricePerBtc))
        return if (sats.signum() > 0 && sats.bitLength() < 63) sats.toLong() else null
    }

    /** The rolls: digits 1 to 6, spaces and commas between them allowed; null when anything else is there. */
    fun rolls(text: String): ByteArray? {
        val out = ArrayList<Byte>()
        for (c in text) {
            when (c) {
                in '1'..'6' -> out.add((c - '0').toByte())
                ' ', ',' -> {}
                else -> return null
            }
        }
        return out.toByteArray()
    }

    /** The screens of four words, 0 to PAGES - 1, and the one after [page]; null after the last. */
    fun pageAfter(page: Int, pages: Int = OwnerStarter.PAGES.toInt()): Int? = if (page + 1 < pages) page + 1 else null
}

/** What the library decides about the limit, the date and the key's deletion (SS-35 to SS-38, SS-43). */
interface StarterCore {
    fun limitState(held: Long, limit: Long, now: Long, moveBy: Long, notYetUntil: Long?): StarterLimit
    fun changeHeld(state: StarterLimitKind, change: StarterChangeKind): Boolean
    fun offersBitcoin(stopAboveLimit: Boolean, held: Long, limit: Long): Boolean
    fun seedMayGo(moveConfirmations: Long): Boolean
    fun notYet(now: Long, askedSecs: Long): Long

    /** The words the Rust core writes: the home line, a state of the limit, a change that waits, the deleted key. */
    fun lineWords(): Words
    fun stateWords(state: StarterLimitKind): Words?
    fun heldWords(): Words
    fun deletedWords(): Words
}

/** The Rust core, through the binding. */
object LibraryStarter : StarterCore {
    override fun limitState(held: Long, limit: Long, now: Long, moveBy: Long, notYetUntil: Long?) =
        OwnerStarter.limitState(held, limit, now, moveBy, notYetUntil)

    override fun changeHeld(state: StarterLimitKind, change: StarterChangeKind) = OwnerStarter.changeHeld(state, change)
    override fun offersBitcoin(stopAboveLimit: Boolean, held: Long, limit: Long) = OwnerStarter.offersBitcoin(stopAboveLimit, held, limit)
    override fun seedMayGo(moveConfirmations: Long) = OwnerStarter.seedMayGo(moveConfirmations)
    override fun notYet(now: Long, askedSecs: Long) = OwnerStarter.notYet(now, askedSecs)
    override fun lineWords() = OwnerStarter.lineWords()
    override fun stateWords(state: StarterLimitKind) = OwnerStarter.stateWords(state)
    override fun heldWords() = OwnerStarter.heldWords()
    override fun deletedWords() = OwnerStarter.deletedWords()
}

/** What the home line and the upgrade screen show (SS-36, SS-37). */
class HomeLine(
    val state: StarterLimitKind,
    val words: SayWords,
    val facts: List<Pair<String, String>>,
    val opensUpgrade: Boolean,
    /** SS-36: what the wallet holds and its limit, in satoshis, and the days left to the date (see [StarterHome.daysLeft]). */
    val held: Long = 0,
    val limit: Long = 0,
    val daysLeft: Long = 0,
    /** The record could not be read, so the wallet is treated as past its date (SS-37): the facts hold no limit or date and "Not yet" is not offered. */
    val unreadable: Boolean = false,
)

object StarterHome {
    /** SS-37: the owner phone opens on the upgrade screen when the wallet is at its limit or past its date, and "Not yet" is not in force. */
    fun opensUpgrade(state: StarterLimitKind): Boolean = state == StarterLimitKind.AtLimit || state == StarterLimitKind.PastDate

    /** SS-37: whether "Not yet" may be taken: in the states the wallet's changes wait in (at the limit, past the date), and only with a record to keep it in. */
    fun mayWaitNotYet(line: HomeLine): Boolean = !line.unreadable && opensUpgrade(line.state)

    /** The day a time falls on, as the app writes a date. */
    fun date(secs: Long): String = Instant.ofEpochSecond(secs).atZone(ZoneOffset.UTC).toLocalDate().toString()

    /** SS-36: the days left to [moveBy] from [now], counting a part of a day as a day; 0 once the date is past. */
    fun daysLeft(now: Long, moveBy: Long): Long = if (moveBy <= now) 0 else (moveBy - now + 86_399) / 86_400

    /** SS-36: "{days} days left", or the last-day words when one day or none is left. */
    fun daysLeftText(days: Long): String =
        if (days <= 1) StarterWords.lineLastDay else StarterWords.lineDaysLeft.replace("{days}", days.toString())

    /** SS-36: what the card says, with the numbers as the home line writes them. */
    fun cardText(held: Long, limit: Long, days: Long): String =
        StarterWords.cardText.replace("{held}", Format.sats(held)).replace("{limit}", Format.sats(limit)).replace("{days}", daysLeftText(days))

    /**
     * SS-36: the line that cannot be dismissed: the library's line words, or the words of the state it is in, with
     * what the wallet holds, the limit and the date as numbers. [lineWords] and [stateWords] are the library's, as they
     * arrive.
     */
    fun line(
        core: StarterCore,
        record: StarterRecord,
        held: Long,
        now: Long,
        lineWords: SayWords,
        counted: Boolean = true,
        stateWords: (StarterLimitKind) -> SayWords?,
    ): HomeLine {
        val limit = core.limitState(held, record.limitSats, record.judged(now), record.moveBy, record.notYetUntil)
        val facts = ArrayList<Pair<String, String>>()
        // Before the node has checked the chain the wallet is not counted: what it holds is not said.
        if (counted) facts.add(StarterWords.lineHeld to Format.sats(held))
        facts.add(StarterWords.lineLimit to Format.sats(record.limitSats))
        facts.add(StarterWords.lineMoveBy to date(record.moveBy))
        limit.until?.let { facts.add(StarterWords.lineNotYetUntil to date(it)) }
        return HomeLine(
            limit.state, stateWords(limit.state) ?: lineWords, facts, opensUpgrade(limit.state),
            held, record.limitSats, daysLeft(record.judged(now), record.moveBy),
        )
    }

    /**
     * SS-37: the line for a starter wallet whose record is missing or cannot be read: it is treated as past its date, so every
     * hold applies and the upgrade screen opens, with the words that say why.
     */
    fun unreadLine(held: Long, counted: Boolean): HomeLine = HomeLine(
        StarterLimitKind.PastDate, StarterWords.said(StarterWords.recordUnread, Tones.STOP),
        if (counted) listOf(StarterWords.lineHeld to Format.sats(held)) else emptyList(),
        true, held, 0, 0, unreadable = true,
    )
}

/** SS-36: when the card that asks for the move is shown. */
object StarterCard {
    private const val DAY = 86_400L
    private const val WEEK = 7 * DAY

    /** The part of the limit held from which the card is shown every day. */
    const val HALF = 0.5

    /** The part of the limit held from which the card is shown at every start of the app. */
    const val NEARLY_FULL = 0.8

    /** The gap between two showings: a day in the last 7 days before the date or from half the limit, else a week. */
    fun interval(nowSecs: Long, dateSecs: Long, heldFraction: Double): Long =
        if (dateSecs - nowSecs <= WEEK || heldFraction >= HALF) DAY else WEEK

    /**
     * SS-36: whether the card is shown now. At every start of the app once the wallet holds 80% of its limit. Otherwise at
     * most once a week; at most once a day in the last 7 days before the date or once the wallet holds half its limit. A
     * card never shown is due. "Not now" holds the card until [notNowUntilSecs], and no longer than a day once the daily
     * rule applies.
     */
    fun cardDue(nowSecs: Long, dateSecs: Long, heldFraction: Double, lastShownSecs: Long?, notNowUntilSecs: Long?, isAppStart: Boolean): Boolean {
        if (heldFraction >= NEARLY_FULL && isAppStart) return true
        if (lastShownSecs == null || lastShownSecs > nowSecs) return true
        // A "Not now" further ahead than the longest gap and a day was not made by this rule, or the clock moved: it is past.
        val notNowUntilSecs = notNowUntilSecs?.takeIf { it <= nowSecs + 8 * DAY }
        val gap = interval(nowSecs, dateSecs, heldFraction)
        val held = if (gap == DAY) lastShownSecs + DAY else maxOf(lastShownSecs + WEEK, notNowUntilSecs ?: 0)
        return nowSecs >= held
    }

    /** What the wallet holds as a part of its limit; 1 for a limit of none. */
    fun fraction(held: Long, limit: Long): Double = if (limit <= 0) 1.0 else held.toDouble() / limit

    /** SS-36: the end of a "Not now" tapped at [nowSecs]: the next time the card is due, not past the start of the last 7 days. */
    fun notNowUntil(nowSecs: Long, dateSecs: Long, heldFraction: Double): Long {
        val next = nowSecs + interval(nowSecs, dateSecs, heldFraction)
        val lastWeek = dateSecs - WEEK
        return if (lastWeek > nowSecs && lastWeek < next) lastWeek else next
    }
}

/**
 * SS-10: the second showing of the words, for a second card, between the check and the sealing. Offered once; not after
 * the key is sealed or the showing was used. Whether it was made is kept in the record ([StarterRecord.secondCopyMade]).
 */
class SecondCopy {
    private var used = false
    private var sealed = false

    /** Whether the offer is shown. */
    fun offered(): Boolean = !used && !sealed

    /** The owner takes the offer: true once, and not after the sealing. */
    fun begin(): Boolean {
        if (!offered()) return false
        used = true
        return true
    }

    /** The owner wrote the second card: the copy was made. */
    var made = false
        private set

    fun finish() {
        if (used && !sealed) made = true
    }

    fun seal() {
        sealed = true
    }
}

/** SS-37: "Not yet" needs a sentence typed exactly, and lasts at most the library's 7 days. */
object StarterNotYet {
    const val MAX_SECS = 7L * 24 * 60 * 60

    /** The sentence is typed exactly: no letter, space or case is forgiven. */
    fun matches(typed: String, sentence: String): Boolean = typed == sentence

    /** The end of a "Not yet" taken at [now]: the library's, asked for the most. Null when the sentence is not the one. */
    fun grant(core: StarterCore, typed: String, sentence: String, now: Long): Long? =
        if (matches(typed, sentence)) core.notYet(now, MAX_SECS) else null
}

/** SS-11: the in-app keypad; its keys go to the library as they are and only counts come back. */
object StarterKeypad {
    val rows = listOf("qwertyuiop", "asdfghjkl", "zxcvbnm")
    const val BACKSPACE = 8
    const val ENTER = 13

    fun letterKey(c: Char): Int {
        require(c in 'a'..'z') { "a key of the keypad is a letter" }
        return c.code
    }

    /** What a key's press shows: the count of letters typed and the step. Never a letter. */
    class Counts(val typed: Long, val accepted: Boolean, val step: Long)

    fun counts(t: org.openbmt.owner.StarterTyped) = Counts(t.typed, t.accepted, t.step)

    /** The position asked now in the check: the one the library picked for step [step], or null after the last. */
    fun asked(positions: List<Long>, step: Long): Long? = positions.getOrNull(step.toInt())
}

/** What the phone has of the starter wallet, and so what it should do (SS-20, SS-23, SS-43). */
enum class StarterKeyState { None, Ready, KeyGone }

object StarterState {
    /** No files: nothing was made (or the key was deleted). Files and a Keystore key: ready. Files and no key: the restore. */
    fun of(hasSealed: Boolean, hasWrapped: Boolean, keyUsable: Boolean): StarterKeyState = when {
        !hasSealed && !hasWrapped -> StarterKeyState.None
        hasSealed && hasWrapped && keyUsable -> StarterKeyState.Ready
        else -> StarterKeyState.KeyGone
    }
}

/**
 * The starter wallet as the shop's pinned own wallet (SS-30, SS-41, CR-256). The library makes the wallet's file from the
 * starter wallet's public description (`pin`); the own store keeps it sealed, with an owner root made the first time. Only
 * the public description is used: no seed and no data key reaches the own store. Whether the shop is a starter shop is
 * not kept apart: it is true when the pinned file is the file of the starter wallet's description.
 */
object StarterShop {
    /**
     * Keeps [wallet] as the shop's pinned wallet. Called after the starter store holds the sealed seed, the wrapped key, the
     * public wallet and the record: the own wallet's file is the last write, and it is what makes the shop whole, so a stop
     * before it leaves a starter wallet with no shop, which the first run finishes the next time "Start simple" is chosen,
     * and never a shop with no starter wallet.
     */
    fun keep(wallet: StarterWalletView, own: OwnShopStore, library: OwnLibrary) {
        val view = library.pin(wallet.description)
        try {
            own.keep(view.file, library)
        } finally {
            view.file.fill(0)
        }
    }

    /** True when the pinned file of the shop is the starter wallet's. False with no starter wallet, no pinned wallet or a file that does not read. */
    fun pinned(starter: StarterStore, own: OwnShopStore, library: OwnLibrary): Boolean = try {
        val mine = starter.loadWallet()
        if (mine == null || !own.hasWallet()) {
            false
        } else {
            val held = own.loadWallet()
            val view = library.pin(mine.description)
            try {
                held != null && held.contentEquals(view.file)
            } finally {
                held?.fill(0)
                view.file.fill(0)
            }
        }
    } catch (_: OwnerException) {
        false
    } catch (_: IOException) {
        false
    } catch (_: LinkageError) {
        false
    }
}

/** The wrapped data key as kept: one byte of IV length, the IV, then the ciphertext. */
object StarterWrap {
    fun pack(iv: ByteArray, ciphertext: ByteArray): ByteArray {
        require(iv.size in 1..255) { "an IV is 1 to 255 bytes" }
        return byteArrayOf(iv.size.toByte()) + iv + ciphertext
    }

    class Parts(val iv: ByteArray, val ciphertext: ByteArray)

    fun unpack(bytes: ByteArray): Parts {
        if (bytes.isEmpty()) throw IOException("the wrapped key is empty")
        val n = bytes[0].toInt() and 0xff
        if (n == 0 || bytes.size <= 1 + n) throw IOException("the wrapped key is cut short")
        return Parts(bytes.copyOfRange(1, 1 + n), bytes.copyOfRange(1 + n, bytes.size))
    }
}

/**
 * What the phone keeps of the starter wallet, in a folder of the no-backup directory (SS-22): the sealed seed the core
 * made, the data key wrapped by the Keystore key, the public wallet, the owner's record, and a payment that is signed
 * and not yet confirmed. Nothing here is the seed or the data key in the clear.
 */
class StarterStore(base: File, folder: String = DIR) {
    val dir = File(base, folder)

    private fun file(name: String) = File(dir, name)

    private fun write(name: String, bytes: ByteArray) {
        if (!dir.isDirectory && !dir.mkdirs()) throw IOException("the starter folder could not be made")
        DurableFile.write(dir, file(name), bytes)
    }

    private fun read(name: String): ByteArray? = file(name).takeIf { it.isFile }?.readBytes()

    fun hasSealed() = file(SEALED).isFile
    fun hasWrapped() = file(WRAPPED).isFile
    fun hasWallet() = file(WALLET).isFile
    fun hasRecord() = file(RECORD).isFile
    fun saveSealed(bytes: ByteArray) = write(SEALED, bytes)
    fun loadSealed(): ByteArray? = read(SEALED)
    fun saveWrapped(bytes: ByteArray) = write(WRAPPED, bytes)
    fun loadWrapped(): ByteArray? = read(WRAPPED)

    /** The public side of the wallet: the description, the fingerprint and the first address, one to a line. */
    fun saveWallet(v: StarterWalletView) = write(WALLET, "${v.description}\n${v.fingerprint}\n${v.firstAddress}\n".toByteArray(Charsets.UTF_8))

    fun loadWallet(): StarterWalletView? {
        val lines = read(WALLET)?.toString(Charsets.UTF_8)?.lines() ?: return null
        if (lines.size < 3 || lines[0].isEmpty()) throw IOException("the starter wallet does not read")
        return StarterWalletView(lines[0], lines[1], lines[2])
    }

    fun saveRecord(r: StarterRecord) = write(RECORD, r.toText().toByteArray(Charsets.UTF_8))
    fun loadRecord(): StarterRecord? = read(RECORD)?.let { StarterRecord.parse(it.toString(Charsets.UTF_8)) }

    /** The new wallet's description the move was made to, kept so that the pinned wallet can change to it (SS-41, SS-43). */
    fun saveNewWallet(description: String) = write(NEW_WALLET, description.toByteArray(Charsets.UTF_8))

    fun loadNewWallet(): String? = read(NEW_WALLET)?.toString(Charsets.UTF_8)?.takeIf { it.isNotEmpty() }

    /** The move, signed and given to the node's outbox, that is to be confirmed once (SS-42, SS-43): its ID, then the transaction. */
    fun savePending(paid: StarterPaid) = write(PENDING, (paid.txid + "\n").toByteArray(Charsets.UTF_8) + paid.tx)

    fun loadPendingTxid(): String? = read(PENDING)?.let { b -> String(b, 0, b.indexOf('\n'.code.toByte()).coerceAtLeast(0), Charsets.UTF_8).ifEmpty { null } }

    /** SS-43: the sealed seed and the wrapped key are overwritten and deleted; the public wallet and the record stay. True when both are gone. */
    fun deleteKey(): Boolean {
        for (name in listOf(SEALED, WRAPPED)) {
            val f = file(name)
            if (f.isFile) {
                try {
                    f.writeBytes(ByteArray(f.length().toInt()))
                } catch (_: IOException) {
                }
                f.delete()
            }
        }
        return !file(SEALED).exists() && !file(WRAPPED).exists()
    }

    /** A setup that stopped half way: the sealed seed and the wrapped key are overwritten and deleted, and so are the public wallet and the record if they were written. True when every file is gone. */
    fun dropOrphan(): Boolean {
        val keyGone = deleteKey()
        for (name in listOf(WALLET, RECORD)) file(name).delete()
        return keyGone && !file(WALLET).exists() && !file(RECORD).exists()
    }

    companion object {
        const val DIR = "starter"
        const val SEALED = "seed.sealed"
        const val WRAPPED = "datakey.wrapped"
        const val WALLET = "wallet.txt"
        const val RECORD = "record.txt"
        const val PENDING = "pending.tx"
        const val NEW_WALLET = "new-wallet.txt"

        /** The Keystore key of the starter, apart from every other key of the app. */
        const val KEY_ALIAS = "openbmt-owner-starter-v1"
    }
}

/**
 * A starter setup that was stopped between its writes (the sealed seed or the wrapped key kept, the public wallet or the
 * record not): the key was never used and no address of the wallet was shown, because an address is shown only from the
 * wallet that is the shop's pinned one, which is written after both files. Such files are dropped and the setup starts from
 * the beginning. Never while a setup of this run is still writing, and never when the wallet is the shop's pinned wallet.
 */
object StarterOrphan {
    fun found(hasSealed: Boolean, hasWrapped: Boolean, hasWallet: Boolean, hasRecord: Boolean, busy: Boolean, pinned: () -> Boolean): Boolean {
        if (busy) return false
        if (!hasSealed && !hasWrapped) return false
        if (hasWallet && hasRecord) return false
        return !pinned()
    }
}

/** SS-31: what the owner sees of a payment before they prove they are there, and of the signed one before it goes out. */
object StarterPayModel {
    /** Before the owner authenticates: the address in full, the amount and the fee rate; nothing is signed yet. */
    fun beforeLines(address: String, sats: Long, feeRatePerKvb: Long): List<Pair<String, String>> = listOf(
        StarterWords.payAddressLabel to address,
        StarterWords.payAmountLabel to Format.sats(sats),
        StarterWords.payFeeRate to "${feeRatePerKvb / 1000}.${(feeRatePerKvb % 1000).toString().padStart(3, '0')} sat/vB",
    )

    /** After it is signed and before it goes out: every address in full, every amount, the fee and the change. */
    fun afterLines(paid: StarterPaid): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        out.add(StarterWords.payAddressLabel to paid.destination)
        out.add(StarterWords.payAmountLabel to Format.sats(paid.paid))
        out.add(StarterWords.payFeeLabel to Format.sats(paid.fee))
        paid.changeSats?.let { out.add(StarterWords.payChangeLabel to Format.sats(it)) }
        return out
    }

    /** SS-42: before the owner authenticates, the move says it takes everything to the new wallet. */
    fun moveBeforeLines(newWallet: String, feeRatePerKvb: Long): List<Pair<String, String>> = listOf(
        StarterWords.moveWalletLabel to newWallet,
        StarterWords.payFeeRate to "${feeRatePerKvb / 1000}.${(feeRatePerKvb % 1000).toString().padStart(3, '0')} sat/vB",
    )

    /** SS-43: delete the key only when the library says the move has its one confirmation. */
    fun mayDelete(core: StarterCore, moveConfirmations: Long?): Boolean = moveConfirmations != null && core.seedMayGo(moveConfirmations)
}

/**
 * The words the Rust core has for the home line, the states of the limit, a change that waits and the deleted key
 * (`starter::line_words`, `state_words`, `held_words`, `deleted_words`), asked of a [StarterCore] and shown as they
 * arrive. A call that fails is shown in the words of its failure.
 */
object StarterCoreWords {
    private fun said(ask: () -> Words): SayWords = try {
        ask().say()
    } catch (e: OwnerException) {
        e.sayWords()
    } catch (e: LinkageError) {
        AppWords.libraryMissing(e.message)
    }

    fun line(core: StarterCore): SayWords = said { core.lineWords() }
    fun held(core: StarterCore): SayWords = said { core.heldWords() }
    fun deleted(core: StarterCore): SayWords = said { core.deletedWords() }

    fun state(core: StarterCore, kind: StarterLimitKind): SayWords? = try {
        core.stateWords(kind)?.say()
    } catch (e: OwnerException) {
        e.sayWords()
    } catch (e: LinkageError) {
        AppWords.libraryMissing(e.message)
    }
}
