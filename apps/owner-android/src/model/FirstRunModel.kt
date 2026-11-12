package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile
import org.openbmt.app.Format
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
         * The money a phone's country most likely uses, from the two letters of its locale: the nine of the app,
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

/** The screens of the first run, in the order they come. Back from each is [FirstRun.back]. */
enum class FirstRunStep { Welcome, Shop, Till, Money, OwnAdvice, Why, RealCases, Guide, NotYet }

/** The three choices of where the money goes. */
enum class MoneyChoice { Own, Make, Later }

/**
 * The first run's state in memory: the step, and what has been entered, kept when Back returns to a step. The
 * name, money and till are saved by the screens when Next is pressed; this class only holds them.
 */
class FirstRun(start: ShopPrefs, locale: String) {
    var step = FirstRunStep.Welcome
        private set
    var name = start.name
    var currency: ShopCurrency = start.currency ?: ShopPrefs.guessCurrency(locale)
    var till: Till? = start.till
    var query = ""
    var money: MoneyChoice? = null

    /** The key maker guide, made new each time the why screen is left for it. Held in memory only. */
    var guide = KeyGuide()
        private set

    fun prefs(stepsShown: Boolean): ShopPrefs = ShopPrefs(ShopPrefs.clean(name), currency, till, stepsShown)

    /**
     * The step after [step]; the Money step goes on to Why for the savings wallet, to OwnAdvice for the own wallet and to
     * NotYet for the others. OwnAdvice goes on to NotYet, or to Why once the choice is changed to the savings wallet.
     */
    fun next() {
        step = when (step) {
            FirstRunStep.Welcome -> FirstRunStep.Shop
            FirstRunStep.Shop -> FirstRunStep.Till
            FirstRunStep.Till -> FirstRunStep.Money
            FirstRunStep.Money -> when (money) {
                MoneyChoice.Make -> FirstRunStep.Why
                MoneyChoice.Own -> FirstRunStep.OwnAdvice
                else -> FirstRunStep.NotYet
            }
            FirstRunStep.OwnAdvice -> if (money == MoneyChoice.Make) FirstRunStep.Why else FirstRunStep.NotYet
            FirstRunStep.Why -> {
                guide = KeyGuide()
                FirstRunStep.Guide
            }
            FirstRunStep.RealCases -> FirstRunStep.RealCases
            FirstRunStep.Guide -> FirstRunStep.NotYet
            FirstRunStep.NotYet -> FirstRunStep.NotYet
        }
    }

    /** The real cases, one tap from the why screen; from no other step. Back returns to the why screen. */
    fun openRealCases() {
        if (step == FirstRunStep.Why) step = FirstRunStep.RealCases
    }

    fun choose(choice: MoneyChoice) {
        money = choice
        next()
    }

    /** From the advice on an own wallet: the owner would rather make a cold wallet, so the savings wallet path follows. */
    fun makeInstead() {
        if (step == FirstRunStep.OwnAdvice) choose(MoneyChoice.Make)
    }

    /** The step before: what was entered stays. Welcome has none. */
    fun back() {
        step = when (step) {
            FirstRunStep.Welcome -> FirstRunStep.Welcome
            FirstRunStep.Shop -> FirstRunStep.Welcome
            FirstRunStep.Till -> FirstRunStep.Shop
            FirstRunStep.Money -> FirstRunStep.Till
            FirstRunStep.OwnAdvice -> FirstRunStep.Money
            FirstRunStep.Why -> FirstRunStep.Money
            FirstRunStep.RealCases -> FirstRunStep.Why
            FirstRunStep.Guide -> if (guide.back()) FirstRunStep.Guide else FirstRunStep.Why
            FirstRunStep.NotYet -> when (money) {
                MoneyChoice.Make -> FirstRunStep.Why
                MoneyChoice.Own -> FirstRunStep.OwnAdvice
                else -> FirstRunStep.Money
            }
        }
    }

    /** Back to the welcome: the owner has chosen Set up my shop again, or left the practice. */
    fun toWelcome() {
        step = FirstRunStep.Welcome
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
 * The key maker guide of the first run (the savings wallet): the steps one to a screen, then the comparison of
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
