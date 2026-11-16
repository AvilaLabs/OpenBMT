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
    const val welcomeLine = "Your keys. Your bitcoin. Your shop."
    const val welcomeNew = "OpenBMT is new, and no independent security audit has been done yet. Start with small amounts."
    const val welcomeTry = "Try it first"
    const val welcomeSetup = "Set up my shop"
    const val welcomeTryNote = "A practice shop on this phone. No money, no wallet, nothing to buy."

    // ---- before starting ---------------------------------------------------------------------------

    const val beforeTitle = "Before you start"
    const val beforeText = "About 10 minutes. You'll need this phone, a pen, and the phone or tablet for your till, if you use one."
    const val beforeResume = "You can stop at any step and carry on later."
    const val beforeGo = "Start"

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
    const val practiceDoneText = "Setting up your shop takes about ten minutes, and you can take a first sale today."
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
    const val tillReads = "OpenBMT only reads your till's orders, to match each bitcoin payment to its sale. It never changes them, and your till never sees a bitcoin address."
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
        "Name it OpenBMT, not Bitcoin: Square's own Bitcoin method pays into Square's wallet.",
        "Turn on Activate on Point of Sale, then tap Add.",
        "To keep all bitcoin in the shop's wallet, turn off Square's own: in the Square app, Settings, Checkout, Payment, then the X by Bitcoin.",
    )
    const val tillSquareCashier = "Cashier: Charge, then More, then Custom payment methods, then OpenBMT."
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

    const val moneyTitle = "Your wallet"
    const val moneyText = "Bitcoin from your sales goes straight to a wallet you hold. Choose how to start."
    const val moneyPlan = "Sell today, then move to a hardware wallet"
    const val moneyPlanToday = "Today: a starter wallet on this phone, with a limit you choose."
    const val moneyPlanLater = "When your hardware wallet arrives: everything moves to it in one step."
    const val moneyOwn = "I already have a wallet"
    const val moneyOwnNote = "A hardware wallet, a multisig, or any wallet that can show its public description."
    const val moneyOther = "Other ways to make one"
    const val moneyMake = "Make a savings wallet on a spare laptop"
    const val moneyMakeNote = "Offline, from your own coin flips. About an hour, guided step by step."
    const val moneyDiy = "A signer you build yourself"
    const val moneyDiyNote = "Made from parts and free software. It connects like any wallet you already have."
    const val moneyLater = "Set up the rest now, add the wallet later"
    const val moneyLaterNote = "The counter says \"Not taking bitcoin yet\" until you do."

    // ---- advice on an own wallet ----------------------------------------------------------------------

    const val ownAdviceTitle = "A wallet for the shop's money"
    const val ownAdviceText = "Best is a cold wallet: its keys stay on a device that never goes online, such as a hardware wallet or an offline computer, " +
        "and it is used only for the shop."
    const val ownAdviceHot = "A wallet on a phone or an online computer suits small amounts only. " +
        "Its keys sit on a device that can be hacked, so keep savings elsewhere."
    const val ownAdviceMake = "Make a savings wallet on a spare laptop"
    const val ownAdviceGo = "Continue with my wallet"

    /** The screen before the existing setup: this build does not have the three paths yet. */
    const val moneyNotYet = "Next, the shop's wallet is made with the Key app on a second phone."

    // ---- the why ----------------------------------------------------------------------------------

    const val whyTitle = "Why the keys are made offline"
    const val whyText1 = "Any app can be hacked, this one too. So the keys to your savings never touch one."
    const val whyText2 = "You make them yourself, offline, from a coin or a die. From then on, your bitcoin moves only when your words say so."
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
    const val realKeysText = "With nobody in the middle, nobody can undo a mistake either. Whoever has your 12 words can spend the money, and if every copy is lost, nobody can give it back. So the words are made where no one can see them, written by hand, and later copied onto metal plates, which survive fire and water."
    const val realCoinTitle = "Why a coin or a die"
    const val realCoinText = "Phones and computers make random numbers with chips and software, and those have had faults before. A coin or a die has no chip and no software, so nothing can reach into it from outside. The key maker only turns your flips into words, offline, and shows its work so you can check it by hand."
    const val realMore = "More cases, with their sources and how each ended, are in the handbook under \"When someone else holds the money\"."

    // ---- getting a hardware wallet (DESIGN 2.1 step 7) -------------------------------------------------

    const val hwTitle = "Getting a hardware wallet"
    const val hwText = "A small device that keeps your keys away from the internet. Four rules keep a fake or tampered one out of your shop."
    const val hwCost = "Most cost between 60 and 150 US dollars."
    const val hwRule1 = "Buy it only from the maker's own website: type the address yourself, never through an ad or a link. Never a marketplace, a reseller or second-hand."
    const val hwRule2 = "It makes new words in front of you. If it arrives with words, or a card already filled in, someone else has them: send it back."
    const val hwRule3 = "Its software is open for anyone to check."
    const val hwRule4 = "The box arrives sealed."
    const val hwNoBrand = "OpenBMT names no brand. Makers change; these rules do not."
    const val hwHelper = "A page for your helper"
    const val hwHelperNote = "The same rules, and the steps for when it arrives, to print or to show someone you trust."
    const val hwArrives = "When it arrives, tap Move to my hardware wallet on the home screen. The phone guides each step."
    const val hwNext = "Make my starter wallet"
    const val helperTitle = "Helping set up a hardware wallet"
    const val helperIntro = "This shop takes bitcoin with OpenBMT. Its money sits in a small starter wallet on the owner's phone until it moves to a hardware wallet."
    const val helperBuy = "Buying it"
    const val helperArrives = "When it arrives"
    val helperSteps = listOf(
        "Check the box is sealed, then set the device up as new. It shows 12 or 24 words: write them by hand, never a photo.",
        "In the owner app, tap Move to my hardware wallet and show the app the device's public description, by its QR code or a file.",
        "Compare the first receive address on the phone with the one on the device's own screen. They must match exactly.",
        "Each counter shows the same address once; the manager checks it against the device and confirms.",
        "The phone moves everything to the hardware wallet in one payment, then deletes the starter key.",
    )

    // ---- instant payments (DESIGN 2.1 step 12) -----------------------------------------------------------

    const val instantTitle = "Instant payments"
    const val instantText = "Small payments count at once. Bigger ones wait about ten minutes, until the bitcoin network has recorded the payment."
    const val instantLabel = "Count at once up to"
    const val instantTypical = "A typical sale in your shop"
    const val instantZero = "Type 0 to make every payment wait."
    const val instantHigher = "To count more at once, raise it later in Settings."
    const val instantHow1 = "In the first minutes, a dishonest customer could try to send the same bitcoin somewhere else, like a bad note. " +
        "The counter and this phone both watch for it and raise the alarm if they see it."
    const val instantHow2 = "Up to your amount, the goods are handed over at once and you carry that small risk. " +
        "Above it, the sale waits for a block, after which a payment is very hard to undo."
    const val instantDetails = "The exact rules, and the limits with your own node or a Box, are in the handbook under Getting paid."
    const val instantNext = "Next"

    // ---- a first real sale (DESIGN 2.1 step 13) ----------------------------------------------------------

    const val firstSaleTitle = "A first real sale"
    const val firstSaleText = "Ring up a small sale on the counter and pay it from any bitcoin wallet."
    const val firstSaleSkip = "Skip, I'll try it later"
    const val firstSaleWaiting = "Waiting for the payment."
    const val firstSalePaid = "Paid to you, person to person. No bank, no processor."
    const val firstSaleSee = "See it yourself"
    const val firstSaleSeeText = "This is the payment's ID. Look it up in any bitcoin wallet or on your own node."
    const val firstSaleDone = "Done"

    // ---- the home checklist (DESIGN 2.1) -------------------------------------------------------------------

    // ---- the end of the first run -----------------------------------------------------------------------------

    const val readyTitle = "Your shop is ready"
    const val readyText = "The counter takes bitcoin now. Anything still to do is on the home screen."
    const val readyGo = "Go to the home screen"

    const val checklistTitle = "Still to do"
    const val checklistSale = "Make a first sale"
    const val checklistNotTested = "Not tested yet"
    const val checklistCopy = "Write a second copy of your words"
    const val checklistCueCard = "Print the cue card for staff"

    // ---- the staff cue card (DESIGN 2.1) ----

    const val cardScreenTitle = "Cue card for staff"
    const val cardOpen = "Cue card for staff"
    const val cardScreenText = "Print it and keep it by the till, where staff can see it."
    const val cardPrint = "Print"
    const val cardTitle = "Taking bitcoin"
    const val cardTitleShop = "Taking bitcoin at {shop}"
    const val cardRingUp = "Ring up the sale in {till} as usual."
    const val cardRingUpOther = "Ring up the sale in your till as usual."
    const val cardCounter = "Type the amount on the counter and show the customer the code."
    const val cardPaid = "Hand over the goods when the counter shows Paid. A bigger payment can take about ten minutes."
    const val cardTillSquare = "Then, in Square: Charge, then More, then Custom payment methods, then OpenBMT."
    const val cardTillToast = "Then, in Toast: Pay, then Other payments, then Bitcoin."
    const val cardTillStoreHub = "Then pick Bitcoin as the payment in StoreHub. If it is not shown, tap Others."
    const val cardTillPick = "Then pick Bitcoin as the payment in {till}."
    const val cardTillPickOther = "Then pick Bitcoin as the payment in your till."
    const val cardDoubt = "Anything other than Paid: keep the goods and ask the owner."
    const val cardPrintFailed = "The card could not be printed. Try again, or choose Save as PDF on the print screen."

    // ---- three layers (DESIGN 2.1) -------------------------------------------------------------------

    const val howThisWorks = "How this works"
    const val details = "Details"

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
        add("welcomeLine", welcomeLine); add("welcomeNew", welcomeNew)
        add("beforeTitle", beforeTitle); add("beforeText", beforeText); add("beforeResume", beforeResume); add("beforeGo", beforeGo)
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
        add("tillTitle", tillTitle); add("tillText", tillText); add("tillReads", tillReads); add("tillSearch", tillSearch)
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
        add("moneyTitle", moneyTitle); add("moneyText", moneyText); add("moneyPlan", moneyPlan)
        add("moneyPlanToday", moneyPlanToday); add("moneyPlanLater", moneyPlanLater); add("moneyOwn", moneyOwn)
        add("moneyOwnNote", moneyOwnNote); add("moneyOther", moneyOther); add("moneyMake", moneyMake); add("moneyMakeNote", moneyMakeNote)
        add("moneyDiy", moneyDiy); add("moneyDiyNote", moneyDiyNote)
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
        add("hwTitle", hwTitle); add("hwText", hwText); add("hwCost", hwCost); add("hwRule1", hwRule1); add("hwRule2", hwRule2)
        add("hwRule3", hwRule3); add("hwRule4", hwRule4); add("hwNoBrand", hwNoBrand); add("hwHelper", hwHelper)
        add("hwHelperNote", hwHelperNote); add("hwArrives", hwArrives); add("hwNext", hwNext)
        add("helperTitle", helperTitle); add("helperIntro", helperIntro); add("helperBuy", helperBuy); add("helperArrives", helperArrives)
        helperSteps.forEachIndexed { i, t -> add("helperStep${i + 1}", t) }
        add("instantTitle", instantTitle); add("instantText", instantText); add("instantLabel", instantLabel)
        add("instantTypical", instantTypical); add("instantZero", instantZero); add("instantHigher", instantHigher); add("instantHow1", instantHow1)
        add("instantHow2", instantHow2); add("instantDetails", instantDetails); add("instantNext", instantNext)
        add("firstSaleTitle", firstSaleTitle); add("firstSaleText", firstSaleText); add("firstSaleSkip", firstSaleSkip)
        add("firstSaleWaiting", firstSaleWaiting); add("firstSalePaid", firstSalePaid); add("firstSaleSee", firstSaleSee)
        add("firstSaleSeeText", firstSaleSeeText); add("firstSaleDone", firstSaleDone)
        add("readyTitle", readyTitle); add("readyText", readyText); add("readyGo", readyGo)
        add("checklistTitle", checklistTitle); add("checklistSale", checklistSale); add("checklistNotTested", checklistNotTested)
        add("checklistCopy", checklistCopy); add("checklistCueCard", checklistCueCard)
        add("cardScreenTitle", cardScreenTitle); add("cardOpen", cardOpen); add("cardScreenText", cardScreenText)
        add("cardPrint", cardPrint); add("cardTitle", cardTitle); add("cardTitleShop", cardTitleShop)
        add("cardRingUp", cardRingUp); add("cardRingUpOther", cardRingUpOther); add("cardCounter", cardCounter)
        add("cardPaid", cardPaid); add("cardTillSquare", cardTillSquare); add("cardTillToast", cardTillToast)
        add("cardTillStoreHub", cardTillStoreHub); add("cardTillPick", cardTillPick); add("cardTillPickOther", cardTillPickOther)
        add("cardDoubt", cardDoubt); add("cardPrintFailed", cardPrintFailed)
        add("howThisWorks", howThisWorks); add("details", details)
        out.addAll(KeyGuideWords.all())
        return out
    }
}
