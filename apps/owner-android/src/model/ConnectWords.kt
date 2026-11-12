package org.openbmt.owner.app

/**
 * The words of "Connect your wallet", the screens after the advice on an own wallet: the wallet's output
 * descriptor is scanned or pasted and its first address is compared with the owner's wallet. They are written down
 * once and the screens copy them. A description that does not read gives the library's own words (SC-01 to SC-12),
 * which are not repeated here. The list is written to docs/screens/owner-app.md by [AppWords.screensMarkdown].
 * No Android class is used.
 */
object ConnectWords {
    const val introTitle = "Connect your wallet"
    const val introText = "Your wallet can show a public description of itself, called an output descriptor. " +
        "OpenBMT uses it to make a new address for every sale. It cannot spend your money."
    const val introWhere = "Where to find it"
    val introSteps = listOf(
        "Open your wallet on the computer or phone where it lives.",
        "Find Export or Settings, then Output descriptor.",
        "Show it as a code, then scan it with this phone.",
    )
    const val introNote = "Sparrow and Coldcard can show one. A wallet that shows only an xpub or zpub cannot be used yet."
    const val introScan = "Scan the code"
    const val introPaste = "Paste the text"

    const val pasteHint = "Paste the output descriptor here."
    const val pasteCheck = "Check"

    const val addressTitle = "Check the first address"
    const val addressText = "This is the first receive address of the wallet you scanned. " +
        "In your wallet, open the list of addresses and find the first receive address. They must be the same."
    const val addressKindSingle = "Wallet with one key"
    const val addressKindMulti = "Wallet with 2 of 3 keys"
    const val addressSame = "Same address"
    const val addressDifferent = "Different"

    const val differentTitle = "Different wallet"
    const val differentText = "The address is not your wallet's first address, so this is a different wallet, " +
        "or the code was changed on the way. Nothing was saved."
    const val differentNext = "Scan the code again from the right wallet."

    const val checkedTitle = "Wallet checked"
    const val checkedText = "This is the right wallet. Sales will go straight to it, " +
        "and OpenBMT can see what arrives but can never spend it."
    const val checkedNext = "Next"

    fun kind(multisig: Boolean): String = if (multisig) addressKindMulti else addressKindSingle

    /** Every key and its text, in the order of the words file, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        fun add(key: String, text: String) { out.add(key to text) }
        add("introTitle", introTitle); add("introText", introText); add("introWhere", introWhere)
        introSteps.forEachIndexed { i, t -> add("introStep${i + 1}", t) }
        add("introNote", introNote); add("introScan", introScan); add("introPaste", introPaste)
        add("pasteHint", pasteHint); add("pasteCheck", pasteCheck)
        add("addressTitle", addressTitle); add("addressText", addressText)
        add("addressKindSingle", addressKindSingle); add("addressKindMulti", addressKindMulti)
        add("addressSame", addressSame); add("addressDifferent", addressDifferent)
        add("differentTitle", differentTitle); add("differentText", differentText); add("differentNext", differentNext)
        add("checkedTitle", checkedTitle); add("checkedText", checkedText)
        add("checkedNext", checkedNext)
        return out
    }
}
