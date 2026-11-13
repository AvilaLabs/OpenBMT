package org.openbmt.owner.app

import java.io.IOException
import org.openbmt.app.SayWords
import org.openbmt.owner.BoxDelivered
import org.openbmt.owner.BoxFound
import org.openbmt.owner.BoxLink
import org.openbmt.owner.BoxOwnCheck
import org.openbmt.owner.BoxOwnInvitation
import org.openbmt.owner.LinkConfig
import org.openbmt.owner.OwnShopArgs
import org.openbmt.owner.OwnerBoxes
import org.openbmt.owner.OwnerException
import org.openbmt.owner.Route

/**
 * The link of a shop with its own wallet to its Box (BX-18): the Box's address as the owner typed it, the Box's key,
 * and the counters that were paired before the Box and still wait for the relay code (CR-132). It is kept sealed in
 * the own store, and the node is started with it. It holds no secret of the owner's: the relay the counters scan is
 * signed again from the owner root each time it is shown.
 */
class OwnLink(val address: String, val relayKey: ByteArray, val needsRelay: List<ByteArray>) {
    /** The bytes that are sealed: `OWL1`, the key, the address and the counters that wait. */
    fun encode(): ByteArray {
        val a = address.toByteArray(Charsets.UTF_8)
        val out = java.io.ByteArrayOutputStream()
        out.write(MAGIC)
        out.write(relayKey)
        out.write(a.size shr 8)
        out.write(a.size and 0xff)
        out.write(a)
        out.write(needsRelay.size shr 8)
        out.write(needsRelay.size and 0xff)
        for (c in needsRelay) out.write(c)
        return out.toByteArray()
    }

    /** A name for this link that two links share only when they are the same Box at the same address. */
    fun id(): String = address + " " + relayKey.joinToString("") { "%02x".format(it) }

    /** The same link with [counter] taken off the list of those that wait for the relay code. */
    fun without(counter: ByteArray): OwnLink = OwnLink(address, relayKey, needsRelay.filter { !it.contentEquals(counter) })

    /** What the node is started with: the route to the Box, its key and the default intervals; the counters come from the settings. */
    fun config(proxy: String): LinkConfig {
        val hp = HostPort.split(address)
        val route = if (ConnectBoxModel.isOnion(address) && hp != null) Route.Onion(proxy, hp.first, hp.second.toLong()) else Route.Direct(address)
        return LinkConfig(route, relayKey, emptyList(), 0, 0)
    }

    companion object {
        private val MAGIC = byteArrayOf('O'.code.toByte(), 'W'.code.toByte(), 'L'.code.toByte(), 1)
        private const val MAX_COUNTERS = 4_096

        /** The link in [bytes]; a file that is not one throws. */
        fun decode(bytes: ByteArray): OwnLink {
            fun bad(): Nothing = throw IOException("the link to the Box is not readable")
            if (bytes.size < 4 + 32 + 2 || !bytes.copyOf(4).contentEquals(MAGIC)) bad()
            val key = bytes.copyOfRange(4, 36)
            val n = ((bytes[36].toInt() and 0xff) shl 8) or (bytes[37].toInt() and 0xff)
            if (n == 0 || bytes.size < 38 + n + 2) bad()
            val address = String(bytes, 38, n, Charsets.UTF_8)
            val at = 38 + n
            val count = ((bytes[at].toInt() and 0xff) shl 8) or (bytes[at + 1].toInt() and 0xff)
            if (count > MAX_COUNTERS || bytes.size != at + 2 + 32 * count) bad()
            val counters = (0 until count).map { bytes.copyOfRange(at + 2 + 32 * it, at + 34 + 32 * it) }
            return OwnLink(address, key, counters)
        }
    }
}

/** The calls of connecting a Box to a shop with its own wallet, so that the tests can stand in for the library (BX-17, BX-18). */
interface OwnBoxLibrary {
    fun invitation(now: Long): BoxOwnInvitation

    fun check(invitation: String, claim: ByteArray): BoxOwnCheck

    fun welcome(shop: OwnShopArgs, link: BoxLink, invitation: String, claim: ByteArray, usdPerBtc: Long): BoxDelivered

    fun relay(ownerSecret: ByteArray, address: String, relayKey: ByteArray): ByteArray
}

/** The library itself. */
object NativeOwnBoxLibrary : OwnBoxLibrary {
    override fun invitation(now: Long) = OwnerBoxes.ownInvitation(now)

    override fun check(invitation: String, claim: ByteArray) = OwnerBoxes.ownCheck(invitation, claim)

    override fun welcome(shop: OwnShopArgs, link: BoxLink, invitation: String, claim: ByteArray, usdPerBtc: Long) =
        OwnerBoxes.ownWelcome(shop, link, invitation, claim, usdPerBtc)

    override fun relay(ownerSecret: ByteArray, address: String, relayKey: ByteArray) = OwnerBoxes.ownRelay(ownerSecret, address, relayKey)
}

/** Which way "Connect your Box" goes for a shop (OA-40, BX-18). */
enum class ConnectBoxPath { KeyApp, OwnWallet }

object ConnectBoxChoice {
    /** A shop with a setup file keeps the Key app's way; one with its own wallet connects the Box by its own invitation. */
    fun of(setupFile: Boolean, ownWallet: Boolean): ConnectBoxPath =
        if (setupFile || !ownWallet) ConnectBoxPath.KeyApp else ConnectBoxPath.OwnWallet
}

/** The relay of a shop's Box and when the running node must start again to follow it (BX-18, CR-130, CR-132). */
object OwnBoxLink {
    /** The signed relay (`OBMz`) for the kept link, or null when the shop has no Box. A refusal of the library throws. */
    fun relay(shop: OwnShopArgs?, link: OwnLink?, library: OwnBoxLibrary): ByteArray? =
        if (shop == null || link == null) null else library.relay(shop.ownerSecret, link.address, link.relayKey)

    /** The relay for the link kept in [store]; a link file that does not read is a refusal, so that no bundle is made without it. */
    fun relayOf(store: OwnShopStore, library: OwnBoxLibrary = NativeOwnBoxLibrary): ByteArray? {
        val link = try {
            store.loadLink()
        } catch (e: IOException) {
            throw OwnerException("BX-18", "the link to the Box is not readable", 1, null)
        }
        return relay(store.args(), link, library)
    }

    /**
     * Whether the node must start again: the link is read once, when the node starts, so a node started with
     * another link than the one now kept (none, another Box, another address) must. Counters paired later are
     * followed from the settings on each tick and need nothing.
     */
    fun restartNeeded(startedWith: String?, kept: OwnLink?): Boolean = kept?.id() != startedWith
}

/** The screens of connecting a Box, in the order they come. */
enum class OwnBoxStep { Invitation, Address, Words, Rest, Sent, Stopped }

/**
 * Connecting a Box to a shop with its own wallet (BX-17, BX-18): the invitation is made and shown, the Box's claim is
 * fetched by the node and checked by the library against the invitation, the four words are compared, the owner
 * is told what the shop holds when the phone is away (BX-17), and the welcome is sent. Every judgment is the
 * library's. The invitation and the claim live in memory; the link is kept only once the Box has taken its welcome,
 * and a welcome sent again for the same claim is the same bytes, so a link that was not saved is saved by a retry.
 */
class OwnBoxPairing(
    private val store: OwnShopStore,
    private val library: OwnBoxLibrary = NativeOwnBoxLibrary,
) {
    var step = OwnBoxStep.Invitation
        private set

    var invitation: BoxOwnInvitation? = null
        private set

    var checked: BoxOwnCheck? = null
        private set

    /** Whether the phone holds no settings, so the first ones need today's price. */
    var needsRate = false
        private set

    var address = ""
        private set

    var stopped: SayWords? = null
        private set

    var delivered: BoxDelivered? = null
        private set

    private var claim: ByteArray? = null

    /** The invitation, made new (BX-12). The words when the library refused or no wallet is kept. */
    fun start(now: Long): SayWords? {
        if (store.args() == null) return OwnPairWords.noWallet
        forget()
        step = OwnBoxStep.Invitation
        return call { invitation = library.invitation(now) }
    }

    /** Whether the invitation has run out at [now]. */
    fun ended(now: Long): Boolean = invitation?.let { now >= it.expiresAt } ?: true

    /** The invitation is read out on the Box: next is the address. */
    fun toAddress() {
        if (step == OwnBoxStep.Invitation) step = OwnBoxStep.Address
    }

    /** The claim the node fetched from the Box at [at]: the library checks it answers this invitation and gives the four words. */
    fun found(at: String, found: BoxFound): SayWords? {
        val text = invitation?.text ?: return OwnPairWords.codeNotShown
        if (step != OwnBoxStep.Address) return null
        return call {
            checked = library.check(text, found.claim)
            claim = found.claim.copyOf()
            needsRate = found.needsRate
            address = at
            step = OwnBoxStep.Words
        }
    }

    /** "They match": the statement of what the shop holds when the phone is away follows. */
    fun match() {
        if (step == OwnBoxStep.Words) step = OwnBoxStep.Rest
    }

    /** "They differ": nothing was sent and nothing is kept. */
    fun differ(words: SayWords): SayWords {
        forget()
        stopped = words
        step = OwnBoxStep.Stopped
        return words
    }

    /**
     * Sends the welcome (BX-18) and keeps the link. [usdPerBtc] starts the first settings and is ignored when the
     * phone holds some. The call waits on the network, so the caller runs it off the screen thread.
     */
    fun send(usdPerBtc: Long, proxy: String): SayWords? {
        val args = store.args() ?: return OwnPairWords.noWallet
        val text = invitation?.text ?: return OwnPairWords.codeNotShown
        val bytes = claim ?: return OwnPairWords.codeNotShown
        if (step != OwnBoxStep.Rest) return null
        val made = call { delivered = library.welcome(args, ConnectBoxModel.linkFor(address, proxy), text, bytes, usdPerBtc) }
        if (made != null) return made
        val d = delivered ?: return OwnPairWords.codeNotShown
        return try {
            store.saveLink(OwnLink(address, d.boxKey, d.counters))
            step = OwnBoxStep.Sent
            null
        } catch (_: IOException) {
            OwnPairWords.said(OwnPairWords.boxLinkNotSaved)
        }
    }

    /** The counters of the kept link that still wait for the relay code. */
    fun needsRelay(): List<ByteArray> = try {
        store.loadLink()?.needsRelay ?: emptyList()
    } catch (_: IOException) {
        emptyList()
    }

    /** The relay code for the counters that were paired before the Box: the owner's signed relay for the kept link. */
    fun relayCode(): ByteArray? {
        val link = try {
            store.loadLink()
        } catch (_: IOException) {
            null
        }
        return OwnBoxLink.relay(store.args(), link, library)
    }

    /** [counter] has taken the relay code: it leaves the list. */
    fun relayTaken(counter: ByteArray) {
        val link = try {
            store.loadLink()
        } catch (_: IOException) {
            null
        } ?: return
        store.saveLink(link.without(counter))
    }

    /** The screen before; false where the caller leaves. A welcome sent is not undone by Back. */
    fun back(): Boolean {
        when (step) {
            OwnBoxStep.Invitation, OwnBoxStep.Sent, OwnBoxStep.Stopped -> return false
            OwnBoxStep.Address -> step = OwnBoxStep.Invitation
            OwnBoxStep.Words -> {
                checked = null
                claim?.fill(0)
                claim = null
                step = OwnBoxStep.Address
            }
            OwnBoxStep.Rest -> step = OwnBoxStep.Words
        }
        return true
    }

    private fun forget() {
        claim?.fill(0)
        claim = null
        checked = null
        invitation = null
        delivered = null
        stopped = null
        address = ""
        needsRate = false
    }

    private inline fun call(f: () -> Unit): SayWords? = try {
        f()
        null
    } catch (e: OwnerException) {
        e.sayWords()
    } catch (e: LinkageError) {
        AppWords.libraryMissing(e.message)
    }
}
