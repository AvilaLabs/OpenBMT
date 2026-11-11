package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.owner.FirstAddress
import org.openbmt.owner.Registration

/** The three ways a hardware signer is given a file and gives one back (OA-70). */
enum class Transport { Ur, Bbqr, File }

/** One way to carry it, with the words the person reads to pick: [examples] name devices that use it. */
class TransportChoice(val transport: Transport, val label: String, val examples: String)

/** One way to show the wallet to a signer (OA-71). */
enum class RegistrationWay { PlainCode, UrBytes, BbqrText, TextFile, DescriptorCode, CryptoOutput, DescriptorFile }

/** What the owner phone offers for a wallet whose key 2 is a hardware signer, as plain text. */
object HardwareModel {
    /**
     * OA-70: the transports, in the order the screen lists them. The person picks; the examples come from
     * what the signers read and write (`docs/SESSION-RULES.md`, AS-90 to AS-99), and no choice is made for
     * them.
     */
    val transports: List<TransportChoice> = listOf(
        TransportChoice(Transport.Ur, "Animated codes (UR)", "For example Keystone, Passport, Jade, Krux and SeedSigner."),
        TransportChoice(Transport.Bbqr, "Animated codes (BBQr)", "For example the Coldcard Q and SeedSigner."),
        TransportChoice(Transport.File, "A file (.psbt)", "For example the Coldcard Mk4 with a microSD card."),
    )

    /** OA-70: the type a UR of a signer file has. */
    const val UR_PSBT = "crypto-psbt"

    /** OA-70: the BBQr kind of a PSBT, and of a finished transaction. */
    const val BBQR_PSBT = 'P'
    const val BBQR_TRANSACTION = 'T'

    /** OA-70: most characters of a part of a BBQr set, and most bytes of a fragment of a UR, for a screen. */
    const val BBQR_PART_CHARS = 400
    const val UR_FRAGMENT_BYTES = 200

    /**
     * OA-71: the ways to show the wallet's text file (a Coldcard multisig text), in the order of the screen:
     * one plain code when it fits, then the animated codes and a file. The library says whether it fits.
     */
    fun textWays(r: Registration): List<RegistrationWay> =
        (if (r.textFitsPlainQr) listOf(RegistrationWay.PlainCode) else emptyList()) +
            listOf(RegistrationWay.UrBytes, RegistrationWay.BbqrText, RegistrationWay.TextFile)

    /** OA-71: the ways to show the descriptor: a plain code when it fits, a `crypto-output` and a file. */
    fun descriptorWays(r: Registration): List<RegistrationWay> =
        (if (r.descriptorFitsPlainQr) listOf(RegistrationWay.DescriptorCode) else emptyList()) +
            listOf(RegistrationWay.CryptoOutput, RegistrationWay.DescriptorFile)

    /** OA-71: the file name of the wallet's text, `<name>.txt`, with the name the library made. */
    fun textFileName(r: Registration): String = r.name.replace(' ', '_') + ".txt"

    /** OA-71: the file name of the descriptor, `<name>.descriptor.txt`. */
    fun descriptorFileName(r: Registration): String = r.name.replace(' ', '_') + ".descriptor.txt"

    /**
     * OA-72: the line on the wallet screen about the first address: done or skipped with the time the
     * person's own clock gives. [at] is formatted by the screen (seconds since 1970 in, text out).
     */
    fun firstAddressLine(record: FirstAddress?, format: (Long) -> String): String = when {
        record == null -> "The first address has not been checked on the signer."
        record.checked -> "The first address was checked on the signer on ${format(record.at)}."
        else -> "The first address was not checked on the signer. Skipped on ${format(record.at)}."
    }

    /** OA-73: a spend's lines as the signer's screen shows them, for the owner to compare. */
    fun compareLines(payments: List<Pair<String, Long>>, change: Pair<String, Long>?, fee: Long): List<String> {
        val lines = ArrayList<String>()
        for ((address, sats) in payments) lines.add("Pay ${Format.sats(sats)} to $address")
        if (change != null) lines.add("Change ${Format.sats(change.second)} back to the wallet at ${change.first}")
        lines.add("Fee ${Format.sats(fee)}")
        return lines
    }
}

/**
 * The words of the owner app's steps with a hardware signer (OA-70 to OA-73). Refusals come from the library
 * with their own words (`docs/screens/owner.md`: AS-90 to AS-99); these are the app's own steps.
 */
object HardwareWords {
    private fun w(tone: Int, heading: String, what: String, who: Int, next: String) =
        SayWords(tone, who, heading, what, next, null, null, null, null)

    val chooseTransport = w(
        Tones.INFO, "How does it reach the signer",
        "A payment for the signer goes as animated codes, or as a file, and the signer's answer comes back the same way.",
        Who.OWNER,
        "Pick the one your signer uses. The list names some signers as examples.",
    )

    val signerReview = w(
        Tones.INFO, "Check it on the signer",
        "The signer's screen is the second check: it shows each address, each amount and the fee, and what goes back to the wallet.",
        Who.OWNER,
        "Compare every address and amount with this screen, and approve on the signer only if they all match.",
    )

    val registerIntro = w(
        Tones.INFO, "Register the wallet",
        "A signer must be told this wallet before it will sign for it, or show its addresses.",
        Who.OWNER,
        "Show the signer the wallet's text, then compare the first address.",
    )

    val partsMissing = w(
        Tones.INFO, "Part of the code is missing",
        "The animated code is not complete yet; the parts can be scanned in any order.",
        Who.OWNER,
        "Keep scanning until every part has been read.",
    )

    val firstAddressCompare = w(
        Tones.INFO, "Check the first address",
        "Open the address explorer on the signer and compare its first receive address with the one shown here.",
        Who.OWNER,
        "Choose Checked if they match. Do not use the wallet until they do, or choose Skip and do it later.",
    )

    val firstAddressChecked = w(
        Tones.GOOD, "First address checked",
        "The signer shows the same first address as this phone, so it has the same wallet.",
        Who.NOBODY,
        "Nothing to do.",
    )

    val firstAddressSkipped = w(
        Tones.INFO, "First address not checked",
        "The check of the first address was skipped, so nothing yet shows that the signer has the same wallet.",
        Who.OWNER,
        "Do the check from the wallet screen before the shop takes money.",
    )

    val returnedNeeded = w(
        Tones.INFO, "Bring the payment back",
        "Scan the animated code the signer shows, or open the file it saved, after you approved on the signer.",
        Who.OWNER,
        "A finished transaction works too, if the signer finishes it.",
    )

    fun all(): List<Pair<String, SayWords>> = listOf(
        "hardwareChooseTransport" to chooseTransport,
        "hardwareSignerReview" to signerReview,
        "hardwareRegisterIntro" to registerIntro,
        "hardwarePartsMissing" to partsMissing,
        "hardwareFirstAddressCompare" to firstAddressCompare,
        "hardwareFirstAddressChecked" to firstAddressChecked,
        "hardwareFirstAddressSkipped" to firstAddressSkipped,
        "hardwareReturnedNeeded" to returnedNeeded,
    )
}
