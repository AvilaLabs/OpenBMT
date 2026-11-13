package org.openbmt.owner.app

import org.openbmt.owner.OwnerNode
import org.openbmt.owner.RequestAsk
import org.openbmt.owner.WebCall
import org.openbmt.owner.WebOutcome
import org.openbmt.owner.WebPair
import org.openbmt.owner.WebPairing
import org.openbmt.owner.WebState
import org.openbmt.owner.WebStep
import org.openbmt.owner.WebStepKind

/** What one send of a call to the store came to (WO-20). */
sealed class WebFetch {
    /** The store answered: its status and its body, whole. */
    class Answer(val status: Long, val body: ByteArray) : WebFetch()

    /** Nothing came back: a timeout, a connection that failed, a certificate the phone does not accept (WO-06). */
    object NotReached : WebFetch()

    /** The shop has Tor on and Tor is not there: nothing was sent (WO-02). */
    object TorMissing : WebFetch()

    /** The answer was longer than the app reads. */
    object TooLong : WebFetch()
}

/** Sends one call to the store, as it is, over HTTPS to the store's own address (WO-02, WO-06). */
fun interface WebFetcher {
    fun send(call: WebCall): WebFetch
}

/**
 * The sender of the store's calls is not built: the node says what to send and nothing is sent, so a visit ends as one
 * the store was not reached for, and the page says so. The choice of the client that carries the calls, over Tor or
 * not, is the owner's to make.
 */
object NoWebFetcher : WebFetcher {
    override fun send(call: WebCall): WebFetch = WebFetch.NotReached
}

/** The web order calls of the library, so that the visits can be tested with no library. */
interface WebPort {
    fun pairStart(ask: RequestAsk, url: String, shopName: String): WebPair

    fun pairConfirm(ask: RequestAsk): WebState

    fun unpair(ask: RequestAsk): WebState

    fun state(ask: RequestAsk): WebState

    fun step(ask: RequestAsk, force: Boolean): WebStep

    fun reply(ask: RequestAsk, outcome: WebOutcome, status: Long, body: ByteArray): WebState
}

/** The real thing: the node's own calls. */
class LibraryWeb(private val node: OwnerNode) : WebPort {
    override fun pairStart(ask: RequestAsk, url: String, shopName: String): WebPair = node.webPairStart(ask, url, shopName)

    override fun pairConfirm(ask: RequestAsk): WebState = node.webPairConfirm(ask)

    override fun unpair(ask: RequestAsk): WebState = node.webUnpair(ask)

    override fun state(ask: RequestAsk): WebState = node.webState(ask)

    override fun step(ask: RequestAsk, force: Boolean): WebStep = node.webStep(ask, force)

    override fun reply(ask: RequestAsk, outcome: WebOutcome, status: Long, body: ByteArray): WebState =
        node.webReply(ask, outcome, status, body)
}

/** A visit to the store: the node's steps, each call sent and its answer handed back, until the node has no more. */
object WebVisit {
    /** The most calls in one visit: an orders call, a range, and the notices that fit in a minute. */
    const val MAX_CALLS = 40

    /**
     * Runs a visit and returns how many calls were sent. [force] asks for a visit now, without the waiting after a
     * failure, and holds for the first step only.
     */
    fun run(port: WebPort, fetcher: WebFetcher, ask: (() -> RequestAsk), force: Boolean): Int {
        var sent = 0
        var first = force
        while (sent < MAX_CALLS) {
            val step = port.step(ask(), first)
            first = false
            val call = step.call
            if (step.step != WebStepKind.Call || call == null) return sent
            val fetched = fetcher.send(call)
            sent++
            when (fetched) {
                is WebFetch.Answer -> port.reply(ask(), WebOutcome.Answered, fetched.status, fetched.body)
                WebFetch.NotReached -> port.reply(ask(), WebOutcome.NotReached, 0, ByteArray(0))
                WebFetch.TorMissing -> port.reply(ask(), WebOutcome.TorMissing, 0, ByteArray(0))
                WebFetch.TooLong -> port.reply(ask(), WebOutcome.TooLong, 0, ByteArray(0))
            }
            // A call that did not get an answer ends the visit: the node waits before the next.
            if (fetched !is WebFetch.Answer) return sent
        }
        return sent
    }
}

/** What the screens work out of the library's state, kept apart so that it can be tested. */
object WebModel {
    /** What the store page shows: no store, a code that waits for the owner, or a store that is visited. */
    enum class Page { NoStore, Waiting, Paired }

    fun page(state: WebState): Page = when (state.pairing) {
        WebPairing.None -> Page.NoStore
        WebPairing.Waiting -> Page.Waiting
        WebPairing.Paired -> Page.Paired
    }

    /** The two things the owner types to pair: neither may be empty (the library checks the rest, WO-06). */
    fun fieldsReady(url: String, shopName: String): Boolean = url.isNotBlank() && shopName.isNotBlank()

    /** WO-11: the page names the addresses left when the store has fewer than this many. */
    const val LOW_ADDRESSES = 20L

    /** Whether the store runs low on addresses, as the page's line says (WO-11). */
    fun runningLow(state: WebState): Boolean = state.left?.let { it < LOW_ADDRESSES } ?: false

    /** What the paired page shows that a visit can change: the page is made again when this does. */
    fun shownKey(state: WebState): List<Any?> = listOf(
        state.pairing, state.lastVisit, state.failure?.heading, state.retryAt, state.left, state.addressesSent, state.trouble?.heading,
        state.orders, state.waiting, state.told, state.noticesWaiting, state.refused.size, state.off?.heading,
    )

    /** WO-21: a visit the owner asks for is possible when a store is paired. */
    fun canVisit(state: WebState): Boolean = state.pairing == WebPairing.Paired && state.off == null
}
