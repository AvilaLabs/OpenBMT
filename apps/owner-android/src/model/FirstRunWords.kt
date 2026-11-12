package org.openbmt.owner.app

/**
 * The words of the first run: the welcome, the practice shop, the shop, the till and the steps for the till
 * (docs/DESIGN.md section 2.1). They are plain sentences and labels, written down once; the screens copy them
 * and write nothing of their own. `{till}` is the name of the till as listed. The list is written to
 * docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object FirstRunWords {
    // ---- welcome --------------------------------------------------------------------------------

    const val welcomeTitle = "Take bitcoin in your shop."
    const val welcomeText = "It goes straight to your own wallet. Nobody holds it for you, and nobody takes a cut of your sales."
    const val welcomeTry = "Try it first"
    const val welcomeSetup = "Set up my shop"
    const val welcomeTryNote = "A practice shop on this phone. No money, no wallet, nothing to buy."

    // ---- practice shop ----------------------------------------------------------------------------

    const val practiceBanner = "PRACTICE. No real money."
    const val practiceIntroTitle = "Here is a sale, start to finish"
    const val practiceIntroText = "You ring it up, the customer scans the code with their bitcoin wallet, and the sale shows Paid. " +
        "In your shop this runs on the counter, the phone or tablet at the till."
    const val practiceStart = "Ring up a sale"
    const val practiceAmountTitle = "Amount"
    const val practiceCharge = "Charge"
    const val practiceQrTitle = "The customer scans this"
    const val practiceQrText = "In a real sale this code asks for the exact amount in bitcoin, at today's price. " +
        "This one is not real and cannot be paid."
    const val practicePayButton = "Pretend the customer paid"
    const val practicePaidTitle = "Paid"
    const val practicePaidText = "The bitcoin went to your wallet. Hand over the goods."
    const val practiceBooksTitle = "Your books"
    const val practiceBooksText = "Every sale is recorded with its amount in your money and in bitcoin, ready for your bookkeeper."
    const val practiceRefundButton = "Try a refund"
    const val practiceRefundText = "A refund is given in cash from the till straight away, or in bitcoin, " +
        "which you approve later from your own wallet."
    const val practiceRefundDone = "Refunded in cash"
    const val practiceDoneTitle = "That is the whole sale"
    const val practiceDoneText = "Setting up your shop takes about ten minutes. Making a wallet, if you need one, " +
        "takes about an hour and can wait."
    const val practiceSetup = "Set up my shop"
    const val practiceAgain = "Try again"
    const val practiceDelete = "Delete the practice shop"
    const val practiceDeleted = "Practice shop deleted."

    // ---- your shop --------------------------------------------------------------------------------

    const val shopTitle = "Your shop"
    const val shopNameLabel = "Shop name"
    const val shopNameHint = "As customers know it"
    const val shopCurrencyLabel = "Your money"
    const val shopCurrencyNote = "Prices are set in this money and shown in bitcoin at the till."
    const val moneyTabLocal = "Local money"
    const val moneyTabBitcoin = "Bitcoin Standard"
    const val moneyBitcoinTitle = "Prices in sats"
    const val moneyBitcoinNote = "Prices are set in sats, and the books count in sats. No exchange rate is used."
    const val moneyTabsSpoken = "Your money: {tab} chosen. Double tap the other tab to switch."
    const val shopNext = "Next"

    // ---- how you take payments now ------------------------------------------------------------------

    const val tillTitle = "How do you take payments now?"
    const val tillText = "Bitcoin is added to your till as one more way to pay. Nothing else about your till changes."
    const val tillSearch = "Search tills"
    const val tillNone = "No till. I write sales down or use a cash box."
    const val tillOther = "Another till"
    const val tillNext = "Next"

    // ---- till steps ---------------------------------------------------------------------------------

    const val tillStepsTitle = "Add Bitcoin to {till}"
    const val tillStepsIntro = "Do this once, on the computer or tablet where you manage {till}. It takes about two minutes."
    const val tillStepsCashier = "At the till, the cashier rings up the sale in {till} as usual, takes the bitcoin on the counter, " +
        "then picks Bitcoin as the payment in {till}."
    const val tillStepsNoFee = "{till} does not move the money, so it charges no fee for this payment type."
    const val tillStepsChecked = "Steps read on {till}'s own help pages on 11 November 2026. " +
        "Menus change; if yours looks different, look for \"payment types\"."
    const val tillStepsDone = "Done"

    const val tillNoneText = "The counter keeps every sale with its time, amount and who took it. Your books come from there."
    const val tillOtherText = "Most tills let you add your own payment type. In your till's settings, look for " +
        "\"payment types\" or \"payment methods\", add one called Bitcoin, and turn it on for your registers."
    const val tillOtherAsk = "If your till has no such setting, the counter still keeps every sale for your books."

    /** The names and the steps of the seven tills, in the order they are listed. */
    const val tillSquare = "Square"
    val tillSquareSteps = listOf(
        "In Square Dashboard, open Settings, then Payments, then Payment methods.",
        "Open Custom payment methods and tap Add payment method.",
        "Name it Bitcoin. Square may not let you rename it later, so check the spelling.",
        "Turn on Activate on Point of Sale, then tap Add.",
    )
    const val tillSquareCashier = "Cashier: Charge, then More, then Custom payment methods, then Bitcoin."
    const val tillSquareReports = "Reports: bitcoin sales are under Other in the sales summary."

    const val tillToast = "Toast"
    val tillToastSteps = listOf(
        "In Toast Web, open the payment settings and find Other payment options.",
        "Tap Add and name it Bitcoin.",
        "Set Show on POS to Yes, then Save and publish.",
    )
    const val tillToastCashier = "Cashier: Pay, then Other payments, then Bitcoin."

    const val tillShopify = "Shopify POS"
    val tillShopifySteps = listOf(
        "In the Shopify POS app, open Settings, then Payment settings.",
        "Open Custom payment types and tap Add custom payment type.",
        "Name it Bitcoin, save, and turn it on.",
    )
    const val tillShopifyNote = "It is on for every POS device unless you turn it off on one."

    const val tillLoyverse = "Loyverse"
    val tillLoyverseSteps = listOf(
        "In Loyverse Back Office, open Settings, then Payment types.",
        "Tap Add payment type, pick Other, and name it Bitcoin.",
        "Choose the stores that take it, then Save.",
    )
    const val tillLoyverseReports = "Reports: Sales by payment type shows bitcoin on its own line."

    const val tillOdoo = "Odoo"
    val tillOdooSteps = listOf(
        "In Odoo, open Point of Sale, then Configuration, then Payment Methods.",
        "Tap New and name it Bitcoin.",
        "Pick a journal of its own for it, leave Integration as None, and choose the points of sale that take it. Save.",
    )

    const val tillHike = "Hike"
    val tillHikeSteps = listOf(
        "In Hike, open Setup, then Payment Types.",
        "Tap Add Payment Type, name it Bitcoin, choose Custom, and save.",
        "Under Register Availability, choose the registers that take it.",
    )

    const val tillStoreHub = "StoreHub"
    val tillStoreHubSteps = listOf(
        "In StoreHub BackOffice, open Settings, then Payment Options.",
        "Tap Add Payment Option, name it Bitcoin, and choose Custom.",
        "Choose the stores and registers, then Save.",
        "On each register, open the menu and tap Sync, twice.",
    )
    const val tillStoreHubCashier = "Cashier: if Bitcoin is not shown at charge, tap Others."

    // ---- where the money goes -----------------------------------------------------------------------

    const val moneyTitle = "Where should the money go?"
    const val moneyText = "Bitcoin from your sales goes straight to a wallet you hold. OpenBMT never holds it and never makes your keys."
    const val moneyOwn = "Use my own wallet"
    const val moneyOwnNote = "You already have a wallet and want sales to go there."
    const val moneyMake = "Help me make a cold wallet"
    const val moneyMakeNote = "With a coin or a die and your laptop, offline. About an hour."
    const val moneyLater = "Set up the rest now, add the wallet later"
    const val moneyLaterNote = "The counter says \"Not taking bitcoin yet\" until you do."

    // ---- advice on an own wallet ----------------------------------------------------------------------

    const val ownAdviceTitle = "We recommend a cold wallet"
    const val ownAdviceText = "A cold wallet keeps its keys on a device that never connects to the internet, such as a hardware signer or an offline computer. " +
        "Its 12 words are best made from your own coin flips or die rolls."
    const val ownAdviceHot = "A hot wallet, on a phone or an online computer, is fine for small amounts. " +
        "Its keys sit on a device that can be hacked, so keep savings elsewhere."
    const val ownAdviceMake = "Make a cold wallet with a coin or a die"
    const val ownAdviceGo = "Continue with my wallet"

    /** The screen before the existing setup: this build does not have the three paths yet. */
    const val moneyNotYet = "This part is still being built. For now, setup continues with the test setup."

    // ---- the why ----------------------------------------------------------------------------------

    const val whyTitle = "Why we do it this way"
    const val whyText1 = "Any app can be hacked, ours included. So your keys never touch one."
    const val whyText2 = "You make them yourself, offline, with a coin or a die. After that, nobody can move your bitcoin without your words. " +
        "Not even us."
    const val whyText3 = "The inconvenience happens once."
    const val whyRealCases = "Real cases"

    // ---- real cases -------------------------------------------------------------------------------

    const val realTitle = "When someone else holds the money"
    const val realBar = "Real cases"
    const val realCount = "{n} of {total}"
    const val realNext = "Next case"
    const val realToBitcoin = "Your own bitcoin"
    const val realDone = "Done"
    const val realCase1Head = "Six years on, most of their savings are still locked in the banks."
    const val realCase2Head = "Of €400,000 taken from one business, about €24,000 came back."
    const val realCase3Head = "48,000 sellers have still not been paid for their own sales."
    const val realCase4Head = "\$64.9 million in savings. \$11.8 million offered back."
    const val realBitcoinHead = "Your own bitcoin is different"
    const val realIntro = "Money in a bank or a payment app is not in your hands. Someone else decides when you can have it. In each case below, people could not get their own money out, through no fault of their own."
    const val realCase1Place = "Lebanon, since 2019"
    const val realCase1Text = "In 2019 Lebanon's banks stopped letting people take out their own dollars. Someone with \$100,000 saved could get about \$500 a month, paid in local money worth a third as much or less. Six years later most of it is still locked in the banks, and no law says they will get it back."
    const val realCase1Quote = "“I got to a point where I was going to sell my kidney…”"
    const val realCase1Who = "A woman in Beirut, for her sister's cancer treatment"
    const val realCase2Place = "Laiki Bank, Cyprus, 2013"
    const val realCase2Text = "Overnight, the government shut Laiki Bank and took every balance above €100,000 to cover the bank's losses. Nobody asked the account holders. A business with €500,000 there kept €100,000. Of the €400,000 taken, about €24,000 came back in six years."
    const val realCase2Quote = "“We can't make any withdrawals, we can't make deposits, pay our suppliers.”"
    const val realCase2Who = "A florist in Nicosia, with the banks shut"
    const val realCase3Place = "TMON and WeMakePrice, South Korea, 2024"
    const val realCase3Text = "Two large shopping websites held their sellers' money from every sale, then ran out of cash and stopped paying. About 48,000 sellers were cut off from \$900 million they had already earned, about \$19,000 each. The government's answer was to offer them loans: credit against their own unpaid money."
    const val realCase3Quote = "“If the settlement is not done by then, it means bankruptcy.”"
    const val realCase3Who = "A seller of household goods in Seoul"
    const val realCase4Place = "Synapse, United States, 2024"
    const val realCase4Text = "Synapse moved money between finance apps and the banks that held it. When it went bankrupt, accounts were frozen for months. One app's customers had \$64.9 million in their accounts and were offered \$11.8 million back. Deposit insurance did not cover them, because Synapse was not a bank."
    const val realCase4Quote = "“[The bank] was only going to pay us \$500 out of that \$280,000.”"
    const val realCase4Who = "A former teacher who had saved for 15 years for a family home"
    const val realBitcoinText = "Bitcoin in your own wallet is not held by a bank or a payment company. Nobody can put a hold on it, close the account or cap your withdrawals, because there is no account. It moves only when your keys sign."
    const val realKeysTitle = "Why the keys need so much care"
    const val realKeysText = "With nobody in the middle, nobody can undo a mistake either. Whoever has your 12 words can spend the money, and if every copy is lost, nobody can give it back. So the words are made where no one can see them, written by hand, and put on steel."
    const val realCoinTitle = "Why a coin or a die"
    const val realCoinText = "Phones and computers make random numbers with chips and software, and those have had faults before. A coin or a die has no chip and no software, so nothing can reach into it from outside. The key maker only turns your flips into words, offline, and shows its work so you can check it by hand."
    const val realMore = "More cases, with their sources and how each ended, are in the handbook under \"When someone else holds the money\"."

    /** The line of a till's name for [t] inside [text]: every `{till}` becomes [t]. */
    fun with(text: String, t: String): String = text.replace("{till}", t)

    /** The spoken description of the money tabs with the chosen tab [tab] named. */
    fun count(n: Int, total: Int): String = realCount.replace("{n}", n.toString()).replace("{total}", total.toString())

    fun withTab(text: String, tab: String): String = text.replace("{tab}", tab)

    /** Every key and its text, in the order of the words file, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        fun add(key: String, text: String) { out.add(key to text) }
        add("welcomeTitle", welcomeTitle); add("welcomeText", welcomeText); add("welcomeTry", welcomeTry)
        add("welcomeSetup", welcomeSetup); add("welcomeTryNote", welcomeTryNote)
        add("practiceBanner", practiceBanner); add("practiceIntroTitle", practiceIntroTitle)
        add("practiceIntroText", practiceIntroText); add("practiceStart", practiceStart)
        add("practiceAmountTitle", practiceAmountTitle); add("practiceCharge", practiceCharge)
        add("practiceQrTitle", practiceQrTitle); add("practiceQrText", practiceQrText)
        add("practicePayButton", practicePayButton); add("practicePaidTitle", practicePaidTitle)
        add("practicePaidText", practicePaidText); add("practiceBooksTitle", practiceBooksTitle)
        add("practiceBooksText", practiceBooksText); add("practiceRefundButton", practiceRefundButton)
        add("practiceRefundText", practiceRefundText); add("practiceRefundDone", practiceRefundDone)
        add("practiceDoneTitle", practiceDoneTitle); add("practiceDoneText", practiceDoneText)
        add("practiceSetup", practiceSetup); add("practiceAgain", practiceAgain)
        add("practiceDelete", practiceDelete); add("practiceDeleted", practiceDeleted)
        add("shopTitle", shopTitle); add("shopNameLabel", shopNameLabel); add("shopNameHint", shopNameHint)
        add("shopCurrencyLabel", shopCurrencyLabel); add("shopCurrencyNote", shopCurrencyNote)
        add("moneyTabLocal", moneyTabLocal); add("moneyTabBitcoin", moneyTabBitcoin)
        add("moneyBitcoinTitle", moneyBitcoinTitle); add("moneyBitcoinNote", moneyBitcoinNote)
        add("moneyTabsSpoken", moneyTabsSpoken); add("shopNext", shopNext)
        add("tillTitle", tillTitle); add("tillText", tillText); add("tillSearch", tillSearch)
        add("tillNone", tillNone); add("tillOther", tillOther); add("tillNext", tillNext)
        add("tillStepsTitle", tillStepsTitle); add("tillStepsIntro", tillStepsIntro)
        add("tillStepsCashier", tillStepsCashier); add("tillStepsNoFee", tillStepsNoFee)
        add("tillStepsChecked", tillStepsChecked); add("tillStepsDone", tillStepsDone)
        for (t in Till.listed) {
            add(t.wordsKey, t.title)
            t.steps.forEachIndexed { i, s -> add("${t.wordsKey}Step${i + 1}", s) }
            t.cashier?.let { add("${t.wordsKey}Cashier", it) }
            t.reports?.let { add("${t.wordsKey}Reports", it) }
            t.note?.let { add("${t.wordsKey}Note", it) }
        }
        add("tillNoneText", tillNoneText); add("tillOtherText", tillOtherText); add("tillOtherAsk", tillOtherAsk)
        add("moneyTitle", moneyTitle); add("moneyText", moneyText); add("moneyOwn", moneyOwn)
        add("moneyOwnNote", moneyOwnNote); add("moneyMake", moneyMake); add("moneyMakeNote", moneyMakeNote)
        add("moneyLater", moneyLater); add("moneyLaterNote", moneyLaterNote); add("moneyNotYet", moneyNotYet)
        add("ownAdviceTitle", ownAdviceTitle); add("ownAdviceText", ownAdviceText); add("ownAdviceHot", ownAdviceHot)
        add("ownAdviceMake", ownAdviceMake); add("ownAdviceGo", ownAdviceGo)
        add("whyTitle", whyTitle); add("whyText1", whyText1); add("whyText2", whyText2); add("whyText3", whyText3)
        add("whyRealCases", whyRealCases); add("realTitle", realTitle); add("realBar", realBar); add("realCount", realCount); add("realNext", realNext); add("realToBitcoin", realToBitcoin); add("realDone", realDone); add("realCase1Head", realCase1Head); add("realCase2Head", realCase2Head); add("realCase3Head", realCase3Head); add("realCase4Head", realCase4Head); add("realIntro", realIntro)
        add("realCase1Place", realCase1Place); add("realCase1Text", realCase1Text); add("realCase1Quote", realCase1Quote); add("realCase1Who", realCase1Who)
        add("realCase2Place", realCase2Place); add("realCase2Text", realCase2Text); add("realCase2Quote", realCase2Quote); add("realCase2Who", realCase2Who)
        add("realCase3Place", realCase3Place); add("realCase3Text", realCase3Text); add("realCase3Quote", realCase3Quote); add("realCase3Who", realCase3Who)
        add("realCase4Place", realCase4Place); add("realCase4Text", realCase4Text); add("realCase4Quote", realCase4Quote); add("realCase4Who", realCase4Who)
        add("realBitcoinHead", realBitcoinHead); add("realBitcoinText", realBitcoinText); add("realKeysTitle", realKeysTitle); add("realKeysText", realKeysText); add("realCoinTitle", realCoinTitle); add("realCoinText", realCoinText); add("realMore", realMore)
        out.addAll(KeyGuideWords.all())
        return out
    }
}
