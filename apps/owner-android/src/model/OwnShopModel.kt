package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.SayWords
import org.openbmt.app.Sealer
import org.openbmt.app.SealedFile
import org.openbmt.owner.OwnBundleView
import org.openbmt.owner.OwnGapView
import org.openbmt.owner.OwnMade
import org.openbmt.owner.OwnOfferView
import org.openbmt.owner.OwnRequestView
import org.openbmt.owner.OwnShopArgs
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerOwn
import org.openbmt.owner.WaitingCounter

/**
 * What the app keeps for a shop with its own wallet (OA-105): the wallet's file, as the library made it, and the
 * secret of the owner root, each sealed in its own file under the Keystore key [AppFiles.OWN_KEY_ALIAS]; and the
 * folder the library keeps the shop's state in (the allocation book, the settings and the bundles that wait). A
 * save goes through the sealed file's atomic write. Nothing here reads what the library made.
 */
class OwnShopStore(private val dir: File, sealer: Sealer) {
    private val wallet = SealedFile(dir, sealer, AppFiles.OWN_WALLET_FILE)
    private val owner = SealedFile(dir, sealer, AppFiles.OWN_OWNER_FILE)

    /** True when a wallet is kept. */
    fun hasWallet(): Boolean = File(dir, AppFiles.OWN_WALLET_FILE).exists()

    /** The wallet's file, or null when none is kept. A file that does not read or does not open throws. */
    fun loadWallet(): ByteArray? = wallet.load()

    /** Keeps the wallet's file, sealed and written atomically. */
    fun saveWallet(file: ByteArray) = wallet.save(file)

    /** The owner root's secret, or null when none is kept. */
    fun loadOwner(): ByteArray? = owner.load()

    /** Keeps the owner root's secret, sealed and written atomically. */
    fun saveOwner(secret: ByteArray) = owner.save(secret)

    /** The folder of the library's state for this shop, made when it is first needed. */
    fun stateDir(): File = File(dir, AppFiles.OWN_DIR).also {
        if (!it.isDirectory && !it.mkdirs()) throw IOException("the shop's folder could not be made")
    }

    /**
     * The three things every call on the shop is given. The secret is read from its file for the call and is not
     * held: null when no wallet or no owner root is kept.
     */
    fun args(): OwnShopArgs? {
        val file = wallet.load() ?: return null
        val secret = owner.load() ?: return null
        return OwnShopArgs(stateDir().absolutePath, secret, file)
    }

    /**
     * Keeps the wallet's [file] and, the first time, makes the owner root with [library] and keeps its secret. The
     * secret is wiped after it is saved. Both are saved before this returns (OA-105).
     */
    fun keep(file: ByteArray, library: OwnLibrary) {
        if (owner.load() == null) {
            val made = library.make()
            try {
                owner.save(made.secret)
            } finally {
                made.secret.fill(0)
            }
        }
        wallet.save(file)
    }
}

/** The calls on the own-wallet shop that the screens make, so that the tests can stand in for the library. */
interface OwnLibrary {
    fun make(): org.openbmt.owner.OwnOwnerMade

    fun pin(text: String): org.openbmt.owner.OwnWalletView

    fun offer(shop: OwnShopArgs, now: Long): OwnOfferView

    fun request(shop: OwnShopArgs, made: OwnMade, now: Long, request: ByteArray): OwnRequestView

    fun confirm(shop: OwnShopArgs, made: OwnMade, now: Long, request: ByteArray, usdPerBtc: Long, relay: ByteArray?): OwnBundleView

    fun differ(): org.openbmt.owner.Words

    fun waiting(shop: OwnShopArgs): List<WaitingCounter>

    fun bundleAgain(shop: OwnShopArgs, counter: ByteArray): OwnBundleView

    fun delivered(shop: OwnShopArgs, counter: ByteArray)

    fun gap(paid: List<Long>, highestGiven: Long?): OwnGapView
}

/** The library itself. */
object NativeOwnLibrary : OwnLibrary {
    override fun make() = OwnerOwn.make()

    override fun pin(text: String) = OwnerOwn.pin(text)

    override fun offer(shop: OwnShopArgs, now: Long) = OwnerOwn.offer(shop, now)

    override fun request(shop: OwnShopArgs, made: OwnMade, now: Long, request: ByteArray) = OwnerOwn.request(shop, made, now, request)

    override fun confirm(shop: OwnShopArgs, made: OwnMade, now: Long, request: ByteArray, usdPerBtc: Long, relay: ByteArray?) =
        OwnerOwn.confirm(shop, made, now, request, usdPerBtc, relay)

    override fun differ() = OwnerOwn.differ()

    override fun waiting(shop: OwnShopArgs) = OwnerOwn.waiting(shop)

    override fun bundleAgain(shop: OwnShopArgs, counter: ByteArray) = OwnerOwn.bundleAgain(shop, counter)

    override fun delivered(shop: OwnShopArgs, counter: ByteArray) = OwnerOwn.delivered(shop, counter)

    override fun gap(paid: List<Long>, highestGiven: Long?) = OwnerOwn.gap(paid, highestGiven)
}

/** The screens of adding a counter, in the order they come. */
enum class OwnPairStep { Offer, Compare, Bundle, Paired, Stopped }

/**
 * Adding a counter to a shop with its own wallet (OA-108 to OA-111): the offer is made and drawn, the counter's
 * request is read and its four words are compared, "They match" makes the bundle, and the bundle is drawn until the
 * owner says the counter shows Paired. Every judgment is the library's: the offer and the request are checked by it
 * (OA-108, OA-109), the settings, the first range and the signed pairing are made by it (OA-110), and it keeps the
 * bundle until it is delivered (OA-111). The offer and the request live in memory for the 10 minutes of the
 * ceremony and are not stored; the secrets are read from [shop] for each call and are not held here.
 */
class OwnPairing(
    private val shop: () -> OwnShopArgs?,
    private val library: OwnLibrary = NativeOwnLibrary,
) {
    var step = OwnPairStep.Offer
        private set

    var offer: OwnOfferView? = null
        private set

    var read: OwnRequestView? = null
        private set

    var bundle: OwnBundleView? = null
        private set

    /** The words of "They differ" once the owner said it, or of a pairing that stopped. */
    var stopped: SayWords? = null
        private set

    private var request: ByteArray? = null

    /** The name the counter gave itself, made safe to show (OA-109). */
    fun name(): String = read?.let { CounterModel.nameOf(it.name) } ?: ""

    /** Whether the 10 minutes of the offer are over at [now] (OA-108). */
    fun ended(now: Long): Boolean = offer?.let { now >= it.endsAt } ?: false

    /** The offer, made new (OA-108); the words when the library refused or no wallet is kept. */
    fun start(now: Long): SayWords? {
        val args = shop() ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_105_NONE)
        step = OwnPairStep.Offer
        read = null
        bundle = null
        stopped = null
        request?.fill(0)
        request = null
        return call { offer = library.offer(args, now) }
    }

    /**
     * The counter's code, read (OA-109): the library checks that it is a request of this shop's offer, signed by the
     * counter it names, and gives its name and the four words. The words when it is refused; the step stays.
     */
    fun readRequest(bytes: ByteArray, now: Long): SayWords? {
        val args = shop() ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_105_NONE)
        val made = offer?.made ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_108)
        if (step != OwnPairStep.Offer) return null
        return call {
            read = library.request(args, made, now, bytes)
            request = bytes.copyOf()
            step = OwnPairStep.Compare
        }
    }

    /** "They match" (OA-110): the bundle is made and stored by the library. [usdPerBtc] is the first settings' price, when asked for. */
    fun match(now: Long, usdPerBtc: Long): SayWords? {
        val args = shop() ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_105_NONE)
        val made = offer?.made ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_108)
        val bytes = request ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_108)
        if (step != OwnPairStep.Compare) return null
        return call {
            bundle = library.confirm(args, made, now, bytes, usdPerBtc, null)
            step = OwnPairStep.Bundle
            request?.fill(0)
            request = null
        }
    }

    /** "They differ" (OA-110): nothing was paired and nothing is kept but the app's own copy, which is forgotten. */
    fun differ(): SayWords {
        val words = try {
            library.differ().say()
        } catch (e: OwnerException) {
            e.sayWords()
        }
        forget()
        stopped = words
        step = OwnPairStep.Stopped
        return words
    }

    /** A counter whose bundle waits (OA-111), shown again exactly as it was made. */
    fun resume(counter: WaitingCounter, now: Long): SayWords? {
        val args = shop() ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_105_NONE)
        return call {
            val b = library.bundleAgain(args, counter.counter)
            bundle = b
            read = OwnRequestView(counter.counter, counter.name, emptyList(), false, b.shares, 0)
            step = OwnPairStep.Bundle
        }
    }

    /** "The counter shows Paired" (OA-111): the library marks the bundle delivered. */
    fun paired(): SayWords? {
        val args = shop() ?: return OwnPairWords.said(OwnPairWords.WORDS_NEEDED_OA_105_NONE)
        val b = bundle ?: return null
        if (step != OwnPairStep.Bundle) return null
        return call {
            library.delivered(args, b.counter)
            step = OwnPairStep.Paired
        }
    }

    /** The counters whose bundle waits (OA-111). */
    fun waiting(): List<WaitingCounter> {
        val args = shop() ?: return emptyList()
        return try {
            library.waiting(args)
        } catch (_: OwnerException) {
            emptyList()
        } catch (_: LinkageError) {
            emptyList()
        }
    }

    /** The screen before; false at the offer, where the caller leaves. A bundle made is kept by the library, so Back from it only leaves. */
    fun back(): Boolean {
        when (step) {
            OwnPairStep.Offer -> return false
            OwnPairStep.Compare -> {
                forget()
                step = OwnPairStep.Offer
            }
            OwnPairStep.Bundle, OwnPairStep.Paired, OwnPairStep.Stopped -> return false
        }
        return true
    }

    private fun forget() {
        request?.fill(0)
        request = null
        read = null
        bundle = null
    }

    private inline fun call(f: () -> Unit): SayWords? = try {
        f()
        null
    } catch (e: OwnerException) {
        e.sayWords()
    } catch (e: LinkageError) {
        AppWords.libraryMissing(e.message)
    }

    companion object {
        /** A counter's request is the shared limit of the Key app's (OA-22): the same code type and size. */
        fun requestBytes(payload: ByteArray): SourceResult = CounterModel.requestResult(payload)
    }
}

/** The gap-limit row of the wallet page (OA-113): its title, what it says and the next step when the wallet must look further. */
class GapRow(val title: String, val line: String, val next: String?)

/** The wallet page of a shop with its own wallet (OA-113). */
object OwnWalletPage {
    /** The row for what the library worked out: the usual line up to 20, the number and the next step above it. */
    fun gapRow(gap: OwnGapView): GapRow =
        if (gap.notice) {
            GapRow(OwnPairWords.gapTitle, OwnPairWords.gapMore.replace("{n}", gap.needed.toString()), OwnPairWords.gapMoreNext)
        } else {
            GapRow(OwnPairWords.gapTitle, OwnPairWords.gapUsual, null)
        }
}
