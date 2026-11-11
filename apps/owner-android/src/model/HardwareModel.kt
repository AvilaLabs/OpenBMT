package org.openbmt.owner.app

import org.openbmt.app.CameraWords
import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.owner.FirstAddress
import org.openbmt.owner.PairView
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
        TransportChoice(Transport.Ur, "Moving QR codes (UR)", "For example Keystone, Passport, Jade, Krux and SeedSigner."),
        TransportChoice(Transport.Bbqr, "Moving QR codes (BBQr)", "For example Coldcard Q and SeedSigner."),
        TransportChoice(Transport.File, "A file on a memory card", "For example Coldcard Mk4 with a microSD card."),
    )

    /** OA-70: the type a UR of a signer file has. */
    const val UR_PSBT = "crypto-psbt"

    /** OA-70: the BBQr kind of a PSBT, and of a finished transaction. */
    const val BBQR_PSBT = 'P'
    const val BBQR_TRANSACTION = 'T'

    /** OA-70: most characters of a part of a BBQr set, and most bytes of a fragment of a UR, for a screen. */
    const val BBQR_PART_CHARS = 400
    const val UR_FRAGMENT_BYTES = 200

    /** OA-70: the types of UR and the kinds of BBQr that are read back from a signer: a PSBT, or a finished transaction. */
    val RETURN_UR_TYPES: Set<String> = setOf(UR_PSBT, "psbt")
    val RETURN_BBQR_KINDS: Set<Char> = setOf(BBQR_PSBT, BBQR_TRANSACTION)

    /** OA-71: the UR type of the descriptor, and of the wallet's text. */
    const val UR_OUTPUT = "crypto-output"
    const val UR_TEXT = "bytes"

    /** OA-71: the BBQr kind of the wallet's text. */
    const val BBQR_TEXT = 'U'

    /** OA-71: the file the wallet's text is saved as is plain text; a PSBT file is bytes. */
    const val MIME_TEXT = "text/plain"
    const val MIME_BYTES = "application/octet-stream"

    /**
     * OA-71: how the wallet's text is shown for the [transport] the person picked: with moving codes of a UR,
     * one plain code when the library says the text fits one, else `ur:bytes`; with moving codes of a BBQr, a
     * set of type U; with a file, the `.txt` file. The descriptor is shown as `ur:crypto-output` whatever the
     * transport (`descriptorWays` lists the other forms the library can make; the screen offers this one).
     */
    fun textWay(transport: Transport, r: Registration): RegistrationWay = when (transport) {
        Transport.Ur -> if (r.textFitsPlainQr) RegistrationWay.PlainCode else RegistrationWay.UrBytes
        Transport.Bbqr -> RegistrationWay.BbqrText
        Transport.File -> RegistrationWay.TextFile
    }

    /** OA-72: whether the pairs of keys the ceremony shows say that key 2 is held on another signer. */
    fun key2IsSigner(pairs: List<PairView>): Boolean = pairs.any {
        (it.keyA == 2L && it.holderA == HOLDER_OTHER_SIGNER) || (it.keyB == 2L && it.holderB == HOLDER_OTHER_SIGNER)
    }

    /** The library's number for a key held on another signer (`CeremonyModel.holder`). */
    const val HOLDER_OTHER_SIGNER = 3

    /** OA-72: an address in groups of 4 characters, for comparing it character by character. */
    fun grouped(address: String): String = address.chunked(4).joinToString(" ")

    /**
     * OA-74: the words of a scan that did not give bytes: those the reading gave, else the camera is off for the
     * app ([off]), else it did not start ([problem] is the system's text).
     */
    fun scanFailure(off: Boolean, problem: String?, say: SayWords?): SayWords =
        say ?: if (off) CameraWords.off(Who.OWNER) else CameraWords.notStarted(Who.OWNER, problem)

    /** What a session of the open approvals is to a wallet whose key 2 is a signer. */
    enum class SignerSession { Exported, NotExported, NotSigner }

    /** The refusal codes of the library that tell a session of spends from one of the Key app (AS-90, AS-92). */
    private const val CODE_NOT_A_SIGNER_SPEND = "AS-90"
    private const val CODE_NOT_EXPORTED = "AS-92"

    /**
     * OA-70: which kind of session it is, from what asking for its file gave: a file (it was exported), the
     * refusal of AS-92 (it holds the spends but nothing was given to the signer) or any other refusal (it is
     * the Key app's).
     */
    fun signerSession(gaveFile: Boolean, refusalCode: String?): SignerSession = when {
        gaveFile -> SignerSession.Exported
        refusalCode == CODE_NOT_EXPORTED -> SignerSession.NotExported
        else -> SignerSession.NotSigner
    }

    /** OA-74: the progress of a set of parts read in any order. */
    fun partsLine(read: Int, of: Int): String = "$read of $of parts read"

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
        Tones.INFO, "How to reach the signer",
        "The payment goes to the signer as moving codes on this screen or as a file, and the signed payment comes back the same way.",
        Who.OWNER,
        "Pick the one your signer uses. The names under each choice are examples.",
    )

    val signerReview = w(
        Tones.INFO, "Check it on the signer",
        "The signer's own screen is the second check. It shows each address, each amount and the fee.",
        Who.OWNER,
        "Compare every address and amount with this screen. Approve on the signer only if they all match.",
    )

    val registerIntro = w(
        Tones.INFO, "Register the wallet",
        "A signer signs for a wallet only after it has been shown the wallet.",
        Who.OWNER,
        "Show the signer this code or file, then check the first address.",
    )

    val partsMissing = w(
        Tones.INFO, "Keep scanning",
        "Some parts of the moving code have not been read yet. They can be read in any order.",
        Who.OWNER,
        "Hold the camera on the signer's screen until every part is read.",
    )

    val firstAddressCompare = w(
        Tones.INFO, "Check the first address",
        "On the signer, open its address list and find the first receive address.",
        Who.OWNER,
        "Compare it with the address shown here, character by character.",
    )

    val firstAddressDiffers = w(
        Tones.STOP, "Do not use with this signer",
        "The signer shows a different first address, so it holds a different wallet or was registered wrongly.",
        Who.OWNER,
        "Register the wallet again on the signer, then check again. If it still differs, do not take money into this wallet.",
    )

    val firstAddressChecked = w(
        Tones.GOOD, "First address checked",
        "The signer shows the same first address as this phone, so it holds the same wallet.",
        Who.NOBODY,
        "Nothing to do.",
    )

    val firstAddressSkipped = w(
        Tones.INFO, "First address not checked",
        "Nothing yet shows that the signer holds the same wallet as this phone.",
        Who.OWNER,
        "Check it from the wallet screen before the shop takes money.",
    )

    val fileSaved = w(
        Tones.INFO, "File saved",
        "The payment for the signer is in the file named below.",
        Who.OWNER,
        "Put the file on the signer's memory card and sign it there. Then bring the signed file back.",
    )

    val returnedNeeded = w(
        Tones.INFO, "Bring the payment back",
        "The signer has the payment. Its signature has to come back to this phone before the payment can be sent.",
        Who.OWNER,
        "Scan the code the signer shows, or open the file it saved. A payment the signer finished itself works too.",
    )

    fun all(): List<Pair<String, SayWords>> = listOf(
        "hardwareChooseTransport" to chooseTransport,
        "hardwareSignerReview" to signerReview,
        "hardwareRegisterIntro" to registerIntro,
        "hardwarePartsMissing" to partsMissing,
        "hardwareFirstAddressCompare" to firstAddressCompare,
        "hardwareFirstAddressDiffers" to firstAddressDiffers,
        "hardwareFirstAddressChecked" to firstAddressChecked,
        "hardwareFirstAddressSkipped" to firstAddressSkipped,
        "hardwareReturnedNeeded" to returnedNeeded,
        "hardwareFileSaved" to fileSaved,
    )
}
