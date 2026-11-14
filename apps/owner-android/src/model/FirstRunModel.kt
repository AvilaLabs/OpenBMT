package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile
import org.openbmt.app.Format
import org.openbmt.owner.Books
import org.openbmt.owner.SettingsEditArgs
import org.openbmt.owner.ShopCurrency

/**
 * What the first run decides and keeps, as plain values (docs/DESIGN.md section 2.1): the tills and their
 * steps, the shop's name, money and till, the guess of the money from the phone's locale, the screens of the
 * first run and of the practice shop and how Back moves between them. The practice shop is state in memory
 * only: nothing here reads or writes a file but [ShopFile], which only the shop's own screens use. No Android
 * class is used.
 */
enum class Till(
    val key: String,
    val wordsKey: String,
    val title: String,
    val steps: List<String> = emptyList(),
    val cashier: String? = null,
    val reports: String? = null,
    val note: String? = null,
) {
    Square("square", "tillSquare", FirstRunWords.tillSquare, FirstRunWords.tillSquareSteps, FirstRunWords.tillSquareCashier, FirstRunWords.tillSquareReports),
    Toast("toast", "tillToast", FirstRunWords.tillToast, FirstRunWords.tillToastSteps, FirstRunWords.tillToastCashier),
    Shopify("shopify", "tillShopify", FirstRunWords.tillShopify, FirstRunWords.tillShopifySteps, note = FirstRunWords.tillShopifyNote),
    Loyverse("loyverse", "tillLoyverse", FirstRunWords.tillLoyverse, FirstRunWords.tillLoyverseSteps, reports = FirstRunWords.tillLoyverseReports),
    Odoo("odoo", "tillOdoo", FirstRunWords.tillOdoo, FirstRunWords.tillOdooSteps),
    Hike("hike", "tillHike", FirstRunWords.tillHike, FirstRunWords.tillHikeSteps),
    StoreHub("storehub", "tillStoreHub", FirstRunWords.tillStoreHub, FirstRunWords.tillStoreHubSteps, FirstRunWords.tillStoreHubCashier),
    None("none", "tillNone", FirstRunWords.tillNone),
    Other("other", "tillOther", FirstRunWords.tillOther),
    ;

    /** Whether the till has steps of its own: the seven that are listed. */
    val listed: Boolean get() = steps.isNotEmpty()

    companion object {
        /** The seven tills, in the order they are listed. */
        val listed: List<Till> = values().filter { it.steps.isNotEmpty() }

        fun fromKey(key: String): Till? = values().firstOrNull { it.key == key }

        /** The tills a search leaves: the listed ones whose name holds [query] in any case, then No till and Another till, always. */
        fun search(query: String): List<Till> {
            val q = query.trim().lowercase()
            return listed.filter { q.isEmpty() || it.title.lowercase().contains(q) } + None + Other
        }
    }
}

/** One page of the till steps: its bar title, then the lines in order, each with how it is set. */
class TillStepsPage(val title: String, val lines: List<Line>, val showsDone: Boolean) {
    enum class Kind { Body, Step, Small }

    class Line(val kind: Kind, val text: String)

    companion object {
        /** The page for [till]: the steps of a listed one, or the text of No till and Another till. */
        fun of(till: Till): TillStepsPage {
            val name = till.title
            val out = ArrayList<Line>()
            return when (till) {
                Till.None -> TillStepsPage(FirstRunWords.tillTitle, listOf(Line(Kind.Body, FirstRunWords.tillNoneText)), true)
                Till.Other -> TillStepsPage(
                    FirstRunWords.tillTitle,
                    listOf(Line(Kind.Body, FirstRunWords.tillOtherText), Line(Kind.Body, FirstRunWords.tillOtherAsk)),
                    true,
                )
                else -> {
                    out.add(Line(Kind.Body, FirstRunWords.with(FirstRunWords.tillStepsIntro, name)))
                    till.steps.forEachIndexed { i, s -> out.add(Line(Kind.Step, "${i + 1}. $s")) }
                    till.note?.let { out.add(Line(Kind.Body, it)) }
                    till.cashier?.let { out.add(Line(Kind.Body, it)) }
                    till.reports?.let { out.add(Line(Kind.Body, it)) }
                    out.add(Line(Kind.Body, FirstRunWords.with(FirstRunWords.tillStepsCashier, name)))
                    out.add(Line(Kind.Body, FirstRunWords.with(FirstRunWords.tillStepsNoFee, name)))
                    out.add(Line(Kind.Small, FirstRunWords.with(FirstRunWords.tillStepsChecked, name)))
                    TillStepsPage(FirstRunWords.with(FirstRunWords.tillStepsTitle, name), out, true)
                }
            }
        }
    }
}

/** What the shop's first screens keep: the name, the money, the till and whether the till steps were shown. */
class ShopPrefs(
    val name: String = "",
    val currency: ShopCurrency? = null,
    val till: Till? = null,
    val stepsShown: Boolean = false,
) {
    fun encode(): ByteArray {
        val sb = StringBuilder()
        sb.append("name: ").append(clean(name)).append('\n')
        sb.append("currency: ").append(currency?.code ?: "").append('\n')
        sb.append("till: ").append(till?.key ?: "").append('\n')
        sb.append("steps-shown: ").append(if (stepsShown) "yes" else "no").append('\n')
        return sb.toString().toByteArray(Charsets.UTF_8)
    }

    fun with(name: String = this.name, currency: ShopCurrency? = this.currency, till: Till? = this.till, stepsShown: Boolean = this.stepsShown): ShopPrefs =
        ShopPrefs(clean(name), currency, till, stepsShown)

    companion object {
        const val MAX_NAME = 60

        /** A name on one line, at most [MAX_NAME] characters, with no control character. */
        fun clean(name: String): String = name.filter { !it.isISOControl() }.trim().take(MAX_NAME)

        fun decode(bytes: ByteArray): ShopPrefs {
            if (bytes.size > 512) throw ConfigFormatException("the shop file is longer than 512 bytes")
            val text = try {
                Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString()
            } catch (_: java.nio.charset.CharacterCodingException) {
                throw ConfigFormatException("the shop file is not UTF-8")
            }
            if (!text.endsWith("\n")) throw ConfigFormatException("the shop file does not end with a line end")
            val lines = text.dropLast(1).split('\n')
            if (lines.size != 4) throw ConfigFormatException("the shop file does not have 4 lines")
            fun value(i: Int, key: String): String {
                val prefix = "$key: "
                if (!lines[i].startsWith(prefix)) throw ConfigFormatException("line ${i + 1} is not \"$key: ...\"")
                return lines[i].substring(prefix.length)
            }
            val name = value(0, "name")
            if (name != clean(name)) throw ConfigFormatException("the name is not a clean line")
            val code = value(1, "currency")
            val currency = if (code.isEmpty()) null else ShopCurrency.values().firstOrNull { it.code == code }
                ?: throw ConfigFormatException("the currency is not one the app has")
            val key = value(2, "till")
            val till = if (key.isEmpty()) null else Till.fromKey(key) ?: throw ConfigFormatException("the till is not one the app lists")
            val shown = when (value(3, "steps-shown")) {
                "yes" -> true
                "no" -> false
                else -> throw ConfigFormatException("steps-shown is not yes or no")
            }
            return ShopPrefs(name, currency, till, shown)
        }

        /**
         * The money a phone's country most likely uses, from the two letters of its locale: the fifteen of the app,
         * the euro for the countries that use it, and the US dollar for any other, as the practice shop uses.
         */
        fun guessCurrency(country: String): ShopCurrency = when (country.uppercase()) {
            "GB" -> ShopCurrency.Gbp
            "JP" -> ShopCurrency.Jpy
            "AU" -> ShopCurrency.Aud
            "IN" -> ShopCurrency.Inr
            "KR" -> ShopCurrency.Krw
            "ID" -> ShopCurrency.Idr
            "SG" -> ShopCurrency.Sgd
            "CA" -> ShopCurrency.Cad
            "MX" -> ShopCurrency.Mxn
            "BR" -> ShopCurrency.Brl
            "TR" -> ShopCurrency.Try
            "TH" -> ShopCurrency.Thb
            "ZA" -> ShopCurrency.Zar
            "AT", "BE", "HR", "CY", "EE", "FI", "FR", "DE", "GR", "IE", "IT", "LV", "LT", "LU", "MT", "NL", "PT", "SK", "SI", "ES" -> ShopCurrency.Eur
            else -> ShopCurrency.Usd
        }
    }
}

/** The shop's file in the app's own folder: the only thing of the first run written to disk, by the shop's screens. Nothing in it is secret. */
class ShopFile(private val dir: File, name: String = AppFiles.SHOP_FILE) {
    private val file = File(dir, name)

    fun load(): ShopPrefs = try {
        if (file.exists()) ShopPrefs.decode(file.readBytes()) else ShopPrefs()
    } catch (_: IOException) {
        // A file that cannot be read is a shop that has not been described yet: the owner types it again.
        ShopPrefs()
    }

    fun save(prefs: ShopPrefs) = DurableFile.write(dir, file, prefs.encode())
}

/**
 * The two tabs of the shop's money: Local money lists the country currencies, Bitcoin Standard (sats) is the tab alone.
 * Choosing the Bitcoin tab sets the currency to sats; going back to Local restores the last local currency picked
 * here, or [guess] (the money of the phone's country) when none was.
 */
class MoneyTabs(start: ShopCurrency, private val guess: ShopCurrency) {
    var currency: ShopCurrency = start
        private set
    private var lastLocal: ShopCurrency = if (start == ShopCurrency.Sats) guess else start

    /** True when the Bitcoin tab is the one shown. */
    val onBitcoin: Boolean get() = currency == ShopCurrency.Sats

    /** The Local tab's list: every currency but sats. */
    val locals: List<ShopCurrency> get() = LOCALS

    fun showBitcoin() {
        currency = ShopCurrency.Sats
    }

    fun showLocal() {
        currency = lastLocal
    }

    /** A pick on the Local list, or a currency set from outside. */
    fun pick(c: ShopCurrency) {
        currency = c
        if (c != ShopCurrency.Sats) lastLocal = c
    }

    companion object {
        val LOCALS: List<ShopCurrency> = ShopCurrency.values().filter { it != ShopCurrency.Sats }
    }
}

/** The screens of the first run, in the order they come. Back from each is [FirstRun.back]. */
enum class FirstRunStep {
    Welcome, Before, Shop, Till, Money, Hardware, Helper, OwnAdvice, Connect, Starter, OwnCounter, Instant, FirstSale, Why, RealCases, Guide, NotYet,
}

/**
 * Where the money goes (docs/DESIGN.md section 2.1 step 6): Plan is the starter wallet now and a hardware wallet
 * later, Own is a wallet the owner already holds, Make is the key maker on a spare laptop and Diy is a signer the
 * owner builds (the last two sit under "Other ways to make one"), Later adds the wallet after the rest.
 */
enum class MoneyChoice { Plan, Own, Make, Diy, Later }

/**
 * The first run's state in memory: the step, and what has been entered, kept when Back returns to a step. The
 * name, money and till are saved by the screens when Next is pressed; this class only holds them.
 */
class FirstRun(start: ShopPrefs, locale: String) {
    var step = FirstRunStep.Welcome
        private set
    var name = start.name
    /** The money's tabs; [currency] is theirs. */
    val moneyTabs = MoneyTabs(start.currency ?: ShopPrefs.guessCurrency(locale), ShopPrefs.guessCurrency(locale))
    var currency: ShopCurrency
        get() = moneyTabs.currency
        set(c) = moneyTabs.pick(c)
    var till: Till? = start.till
    var query = ""
    var money: MoneyChoice? = null

    /** The key maker guide, made new each time the why screen is left for it. Held in memory only. */
    var guide = KeyGuide()
        private set

    /** "Connect your wallet", made new each time the advice is left for it. Held in memory only. */
    var connect = Connect()
        private set

    fun prefs(stepsShown: Boolean): ShopPrefs = ShopPrefs(ShopPrefs.clean(name), currency, till, stepsShown)

    /**
     * The step after [step]; the Money step goes on to Hardware for the plan, to Connect for a wallet the owner holds,
     * to Why for the key maker, to OwnAdvice for a signer built by the owner and to NotYet for the others. The plan's
     * Hardware goes on to Starter. Starter and Connect (once checked) go on to adding the first counter, then to Instant,
     * then to FirstSale, then NotYet. OwnAdvice goes on to Connect, or to Why once the choice is changed to the key maker.
     */
    fun next() {
        step = when (step) {
            FirstRunStep.Welcome -> FirstRunStep.Before
            FirstRunStep.Before -> FirstRunStep.Shop
            FirstRunStep.Shop -> FirstRunStep.Till
            FirstRunStep.Till -> FirstRunStep.Money
            FirstRunStep.Money -> when (money) {
                MoneyChoice.Make -> FirstRunStep.Why
                MoneyChoice.Diy -> FirstRunStep.OwnAdvice
                MoneyChoice.Own -> {
                    connect = Connect()
                    FirstRunStep.Connect
                }
                MoneyChoice.Plan -> FirstRunStep.Hardware
                else -> FirstRunStep.NotYet
            }
            // The buying guide goes on to the starter wallet; the helper's page returns to the guide.
            FirstRunStep.Hardware -> FirstRunStep.Starter
            FirstRunStep.Helper -> FirstRunStep.Hardware
            // The starter wallet, once made, is the shop's pinned wallet: the first counter follows, as after Connect.
            FirstRunStep.Starter -> FirstRunStep.OwnCounter
            FirstRunStep.OwnAdvice -> if (money == MoneyChoice.Make) FirstRunStep.Why else {
                connect = Connect()
                FirstRunStep.Connect
            }
            FirstRunStep.Connect -> if (connect.step == ConnectStep.Checked) FirstRunStep.OwnCounter else FirstRunStep.Connect
            FirstRunStep.OwnCounter -> FirstRunStep.Instant
            FirstRunStep.Instant -> {
                firstSale = FirstSale()
                FirstRunStep.FirstSale
            }
            FirstRunStep.FirstSale -> FirstRunStep.NotYet
            FirstRunStep.Why -> {
                guide = KeyGuide()
                FirstRunStep.Guide
            }
            FirstRunStep.RealCases -> FirstRunStep.RealCases
            FirstRunStep.Guide -> FirstRunStep.NotYet
            FirstRunStep.NotYet -> FirstRunStep.NotYet
        }
    }

    /** The page of the real cases: 0 to 3 are the four cases, [REAL_PAGES] - 1 is the page on the owner's own bitcoin. */
    var realPage = 0
        private set

    /** The real cases, one tap from the why screen; from no other step. They open at the first case. */
    fun openRealCases() {
        if (step == FirstRunStep.Why) {
            step = FirstRunStep.RealCases
            realPage = 0
        }
    }

    /** The next page of the real cases; the last one stays. */
    fun realNext() {
        if (step == FirstRunStep.RealCases && realPage < REAL_PAGES - 1) realPage++
    }

    /** Done on the last page of the real cases: back to the why screen. */
    fun realDone() {
        if (step == FirstRunStep.RealCases) step = FirstRunStep.Why
    }

    /** The helper's page, one tap from the buying guide; from no other step. */
    fun openHelper() {
        if (step == FirstRunStep.Hardware) step = FirstRunStep.Helper
    }

    /** What the owner typed for the instant limit, kept when Back returns to the step; null until something was typed. */
    var instantTyped: String? = null

    /** What the owner typed for a typical sale: shown beside the limit for comparison, and never written. */
    var typicalTyped = ""

    /** The first sale watch of the FirstSale step, made new each time the step is entered. */
    var firstSale = FirstSale()
        private set

    /** What this step queued, so that the same value is not queued twice; null until it queued one. */
    var queuedLimit: Long? = null
        private set

    /**
     * Instant, Next: the limit typed may be 0 up to [held], the limit the shop has (it is lowered or kept here, never
     * raised). It is written with [write] as whole units of the shop's money unless it is what the shop holds or what
     * this step already queued; then the step goes on. With nothing written and the step kept: [InstantResult.NotNumber]
     * for text that is not a whole number, [InstantResult.Higher] for more than [held]. [write] may throw; then the step stays too.
     */
    fun saveInstant(text: String, held: Long, write: (Long) -> Unit): InstantResult {
        if (step != FirstRunStep.Instant) return InstantResult.NotHere
        val v = InstantStep.parse(text) ?: return InstantResult.NotNumber
        if (v > held) return InstantResult.Higher
        if (v != held && v != queuedLimit) {
            write(v)
            queuedLimit = v
        }
        instantTyped = text
        next()
        return InstantResult.Saved
    }

    /** Instant, Next while the starter wallet holds the shop at its limit or date (SS-37): nothing is written and the step goes on. */
    fun skipInstant() {
        if (step == FirstRunStep.Instant) next()
    }

    fun choose(choice: MoneyChoice) {
        money = choice
        next()
    }

    /** From the advice on a signer built by the owner: the owner would rather have the key maker, so its path follows. */
    fun makeInstead() {
        if (step == FirstRunStep.OwnAdvice) choose(MoneyChoice.Make)
    }

    /** The step before: what was entered stays. Welcome has none. */
    fun back() {
        step = when (step) {
            FirstRunStep.Welcome -> FirstRunStep.Welcome
            FirstRunStep.Before -> FirstRunStep.Welcome
            FirstRunStep.Shop -> FirstRunStep.Before
            FirstRunStep.Till -> FirstRunStep.Shop
            FirstRunStep.Money -> FirstRunStep.Till
            FirstRunStep.Hardware -> FirstRunStep.Money
            FirstRunStep.Helper -> FirstRunStep.Hardware
            FirstRunStep.OwnAdvice -> FirstRunStep.Money
            FirstRunStep.Connect -> if (connect.back()) FirstRunStep.Connect else if (money == MoneyChoice.Diy) FirstRunStep.OwnAdvice else FirstRunStep.Money
            FirstRunStep.Starter -> FirstRunStep.Hardware
            FirstRunStep.OwnCounter -> if (money == MoneyChoice.Plan) FirstRunStep.Starter else FirstRunStep.Connect
            FirstRunStep.Instant -> FirstRunStep.OwnCounter
            FirstRunStep.FirstSale -> FirstRunStep.Instant
            FirstRunStep.Why -> FirstRunStep.Money
            FirstRunStep.RealCases -> if (realPage > 0) {
                realPage--
                FirstRunStep.RealCases
            } else {
                FirstRunStep.Why
            }
            FirstRunStep.Guide -> if (guide.back()) FirstRunStep.Guide else FirstRunStep.Why
            FirstRunStep.NotYet -> when (money) {
                MoneyChoice.Make -> FirstRunStep.Why
                MoneyChoice.Own, MoneyChoice.Diy, MoneyChoice.Plan -> FirstRunStep.FirstSale
                else -> FirstRunStep.Money
            }
        }
    }

    /** Back to the welcome: the owner has chosen Set up my shop again, or left the practice. */
    fun toWelcome() {
        step = FirstRunStep.Welcome
    }

    companion object {
        /** The four cases and the page on the owner's own bitcoin. */
        const val REAL_PAGES = 5
    }
}

/** What [FirstRun.saveInstant] did. */
enum class InstantResult { Saved, NotNumber, Higher, NotHere }

/** The page for the owner's helper (DESIGN 2.1 step 7): the four rules for buying and the numbered steps for when it arrives. */
object HelperPage {
    val rules = listOf(FirstRunWords.hwRule1, FirstRunWords.hwRule2, FirstRunWords.hwRule3, FirstRunWords.hwRule4)

    /** The steps, each with its number. */
    val steps: List<String> get() = FirstRunWords.helperSteps.mapIndexed { i, t -> "${i + 1}. $t" }

    /** The whole page as plain text, for the share sheet. */
    fun text(): String = buildString {
        append(FirstRunWords.helperTitle).append("\n\n")
        append(FirstRunWords.helperIntro).append("\n\n")
        append(FirstRunWords.helperBuy).append('\n')
        rules.forEach { append("- ").append(it).append('\n') }
        append('\n').append(FirstRunWords.helperArrives).append('\n')
        steps.forEach { append(it).append('\n') }
    }
}

/** The instant step (DESIGN 2.1 step 12): the shop's light-client limit for one sale. */
object InstantStep {
    /** What the library's policy starts the limit at (policy::LIGHT_PER_SALE_USD), for a shop whose settings are not held yet. */
    const val DEFAULT_PER_SALE = 10L

    /** The limit typed: whole units of the shop's money, 0 allowed (CR-194); null when it is not that. */
    fun parse(text: String): Long? = SettingsEditModel.dollars(text)

    /** The text the field starts with: what the settings hold, else the policy's start. */
    fun initial(held: Long?): String = (held ?: DEFAULT_PER_SALE).toString()

    /** The edit the settings screen would make for [limit]: this one field, as SettingsEdit builds it. */
    fun edit(limit: Long): SettingsEditArgs = SettingsEditArgs(lightPerSale = limit)
}

/**
 * The first sale step (DESIGN 2.1): waiting until the owner phone records a paid sale of the shop, then paid. The
 * count is the books' own (sales that received funds, from the counter's records); nothing here asks the network.
 */
class FirstSale {
    /** The count when the step was entered: only a count above it is a sale of this step. */
    var baseline: Long? = null
        private set

    var paid = false
        private set

    /** The payment's ID once the owner asked to see it and the ledger gave it; null when it is not known. */
    var txid: String? = null
        private set

    /** True once the owner pressed "See it yourself". */
    var seeing = false
        private set

    /** The books' count of paid sales: the first count is the baseline; true, once, when a later one is above it. */
    fun observe(paidSales: Long): Boolean {
        val base = baseline
        if (base == null) {
            baseline = paidSales
            return false
        }
        if (paid || paidSales <= base) return false
        paid = true
        return true
    }

    fun see(id: String?) {
        if (!paid) return
        seeing = true
        txid = id
    }

    companion object {
        /** The paid sales of a books view: every sale that received funds. */
        fun paidSales(b: Books): Long = b.days.sumOf { it.sales }

        /**
         * The ID of the first paid sale in the full ledger's bytes (the "Txid" column), or null when there is none.
         * A sale paid by more than one payment gives the first of them.
         */
        fun firstTxid(ledger: ByteArray): String? {
            val lines = String(ledger, Charsets.UTF_8).split('\n')
            val head = lines.firstOrNull()?.let { cells(it) } ?: return null
            val at = head.indexOf("Txid")
            if (at < 0) return null
            for (line in lines.drop(1)) {
                val id = cells(line).getOrNull(at)?.let { Regex("[0-9a-f]{64}").find(it)?.value }
                if (id != null) return id
            }
            return null
        }

        /** One line of the ledger split at its commas, with the quotes of a cell taken off. */
        private fun cells(line: String): List<String> {
            val out = ArrayList<String>()
            val cur = StringBuilder()
            var quoted = false
            var i = 0
            while (i < line.length) {
                val c = line[i]
                when {
                    quoted && c == '"' && line.getOrNull(i + 1) == '"' -> { cur.append('"'); i++ }
                    c == '"' -> quoted = !quoted
                    c == ',' && !quoted -> { out.add(cur.toString()); cur.setLength(0) }
                    else -> cur.append(c)
                }
                i++
            }
            out.add(cur.toString().trimEnd('\r'))
            return out
        }
    }
}

/** One item of the home checklist: its words and, for the first sale, the note under it. */
class ChecklistItem(val text: String, val note: String? = null)

/** The home checklist (DESIGN 2.1): what is still to do, and gone when nothing is. */
object Checklist {
    /** The second copy is still to do only for a sealed, pinned starter wallet whose record says it was not made. */
    fun copyDue(sealed: Boolean, pinned: Boolean, record: StarterRecord?): Boolean = sealed && pinned && record != null && !record.secondCopyMade

    /**
     * The items for a shop: a first sale until one is paid, and a second copy of the starter words only when the
     * owner skipped it. Empty when both are done.
     */
    fun items(salePaid: Boolean, copySkipped: Boolean): List<ChecklistItem> {
        val out = ArrayList<ChecklistItem>()
        if (!salePaid) out.add(ChecklistItem(FirstRunWords.checklistSale, FirstRunWords.checklistNotTested))
        if (copySkipped) out.add(ChecklistItem(FirstRunWords.checklistCopy))
        return out
    }
}

/** The screens of the key maker guide, in the order they come. The first six are the steps, in the order of the page's. */
enum class GuideStep(val number: Int?) {
    Need(1), Tails(2), Page(3), Start(4), Open(5), Follow(6),

    /** The scanned wallet beside the page's: the fingerprint and the first address. */
    Compare(null),

    /** The same two in Electrum. */
    Electrum(null),

    /** Shut down Tails. */
    Done(null),

    /** A difference was found: stop, and start the guide again. */
    Stopped(null),
}

/**
 * The key maker guide of the first run (the cold wallet): the steps one to a screen, then the comparison of
 * what the page's code gave with the page and with Electrum. It is state in memory only: nothing here reads or
 * writes a file, and no word of the wallet is asked for or shown. What was scanned is the fingerprint and the
 * first address, which hold no secret.
 */
class KeyGuide {
    var step = GuideStep.Need
        private set

    /** The 8 hex digits of the scanned wallet, until the guide ends or starts again. */
    var fingerprint: String? = null
        private set

    /** The first receive address of the scanned wallet. */
    var firstAddress: String? = null
        private set

    /** The next screen of a step. Follow goes on only by a scan ([scanned]); the comparisons go on by their answers. */
    fun next() {
        step = when (step) {
            GuideStep.Need -> GuideStep.Tails
            GuideStep.Tails -> GuideStep.Page
            GuideStep.Page -> GuideStep.Start
            GuideStep.Start -> GuideStep.Open
            GuideStep.Open -> GuideStep.Follow
            GuideStep.Follow, GuideStep.Compare, GuideStep.Electrum, GuideStep.Done, GuideStep.Stopped -> step
        }
    }

    /**
     * The scan gave a wallet: the comparison with the page. A wallet with no fingerprint (more than one key) is
     * not the key maker's: the guide stays on step 6 and false is returned.
     */
    fun scanned(fingerprint: String?, firstAddress: String): Boolean {
        if (step != GuideStep.Follow || fingerprint == null) return false
        this.fingerprint = fingerprint
        this.firstAddress = firstAddress
        step = GuideStep.Compare
        return true
    }

    /** "They are the same" or "Electrum shows the same": on to the next check, then to the end. */
    fun same() {
        step = when (step) {
            GuideStep.Compare -> GuideStep.Electrum
            GuideStep.Electrum -> GuideStep.Done
            else -> step
        }
    }

    /** "They are different" or "Electrum shows something different": stop. */
    fun different() {
        if (step == GuideStep.Compare || step == GuideStep.Electrum) step = GuideStep.Stopped
    }

    /** Back from the stop to the first step, with nothing of the scan kept. */
    fun restart() {
        step = GuideStep.Need
        forget()
    }

    /** The screen before; false at the first step, where the caller leaves the guide. */
    fun back(): Boolean {
        step = when (step) {
            GuideStep.Need -> return false
            GuideStep.Tails -> GuideStep.Need
            GuideStep.Page -> GuideStep.Tails
            GuideStep.Start -> GuideStep.Page
            GuideStep.Open -> GuideStep.Start
            GuideStep.Follow -> GuideStep.Open
            GuideStep.Compare -> GuideStep.Follow
            GuideStep.Electrum -> GuideStep.Compare
            GuideStep.Done -> GuideStep.Electrum
            GuideStep.Stopped -> {
                forget()
                GuideStep.Need
            }
        }
        if (step == GuideStep.Follow) forget()
        return true
    }

    private fun forget() {
        fingerprint = null
        firstAddress = null
    }

    companion object {
        /** The number of steps the badge counts. */
        const val TOTAL = 6

        /** The address in groups of four, as the key maker page shows it. */
        fun groups(address: String): String = address.chunked(4).joinToString(" ")
    }
}

/** The screens of the practice shop, in the order they come. */
enum class PracticeStep { Intro, Amount, Qr, Paid, Books, Refund, Done }

/**
 * The practice shop: a sale from the intro to the end, held in memory only. Nothing is written to a file,
 * nothing is sent, and no call into the library is made. Deleting it is forgetting this object.
 */
class Practice(val currency: ShopCurrency, private val clock: () -> Long) {
    var step = PracticeStep.Intro
        private set

    /** The digits typed on the number pad, with no leading zero. */
    var digits = ""
        private set

    /** When the sale was paid, as seconds since 1970; null before. */
    var paidAt: Long? = null
        private set
    var refunded = false
        private set

    /** The amount typed, in whole units of the money. */
    val amount: Long get() = if (digits.isEmpty()) 0 else digits.toLong()

    fun digit(d: Int) {
        if (step != PracticeStep.Amount || d !in 0..9) return
        if (digits.length >= MAX_DIGITS) return
        if (digits.isEmpty() && d == 0) return
        digits += d.toString()
    }

    fun delete() {
        if (step == PracticeStep.Amount) digits = digits.dropLast(1)
    }

    /** Whether Charge may be pressed: an amount above zero. */
    val canCharge: Boolean get() = amount > 0

    /** The next screen. Charge needs an amount; Paid records the time. */
    fun next() {
        step = when (step) {
            PracticeStep.Intro -> PracticeStep.Amount
            PracticeStep.Amount -> if (canCharge) PracticeStep.Qr else PracticeStep.Amount
            PracticeStep.Qr -> {
                paidAt = clock()
                PracticeStep.Paid
            }
            PracticeStep.Paid -> PracticeStep.Books
            PracticeStep.Books -> PracticeStep.Refund
            PracticeStep.Refund -> {
                refunded = true
                PracticeStep.Done
            }
            PracticeStep.Done -> PracticeStep.Done
        }
    }

    /** The screen before; the amount and the time stay. Intro has none: the caller returns to the welcome. */
    fun back(): Boolean {
        val before = when (step) {
            PracticeStep.Intro -> return false
            PracticeStep.Amount -> PracticeStep.Intro
            PracticeStep.Qr -> PracticeStep.Amount
            PracticeStep.Paid -> PracticeStep.Qr
            PracticeStep.Books -> PracticeStep.Paid
            PracticeStep.Refund -> PracticeStep.Books
            PracticeStep.Done -> PracticeStep.Refund
        }
        if (step == PracticeStep.Done) refunded = false
        step = before
        return true
    }

    /** Another sale from the amount screen: the amount and the books start again. */
    fun again(): Practice = Practice(currency, clock).also { it.next() }

    /** The amount as the books and the pad show it: whole units of the money. */
    fun amountText(): String = if (currency == ShopCurrency.Sats) Format.sats(amount) else SettingsEditModel.unitsText(currency, amount)

    /** The one row of the books: when, how much, and Paid. */
    fun booksRow(): String = "${Format.localTime(paidAt ?: clock())}  ${amountText()}  ${FirstRunWords.practicePaidTitle}"

    companion object {
        /** The text of the code, exactly: no wallet reads it as a payment. */
        const val QR_TEXT = "OPENBMT PRACTICE - NOT A PAYMENT"
        const val MAX_DIGITS = 7
    }
}
