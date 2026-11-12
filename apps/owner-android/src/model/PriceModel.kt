package org.openbmt.owner.app

import org.openbmt.owner.AgreedPrice
import org.openbmt.owner.AgreedPriceIn
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerPrice
import org.openbmt.owner.PriceReply
import org.openbmt.owner.PriceSource

/** The numbers of docs/RATE-RULES.md, as the app reads them. A test of the Rust crate checks that they are the same. */
object PriceLimits {
    /** RT-43: the seconds after which a fetch is given up. */
    const val FETCH_TIMEOUT_SECS = 20L

    /** RT-10: the most bytes of a reply that are read. */
    const val MAX_RESPONSE_BYTES = 64 * 1024
}

/** What one fetch of a source gave: the reply, whole, or why there is none (RT-44). */
sealed class RateFetch {
    class Body(val bytes: ByteArray) : RateFetch()

    /** No reply: a timeout, a connection that failed, a status other than 200, a reply that was too long. */
    object NotAnswered : RateFetch()

    /** The proxy did not answer: nothing was sent (OA-05). */
    object TorMissing : RateFetch()
}

/** Fetches one source's reply. The Android one is `HttpsRateFetcher`; the tests use a fake. */
fun interface RateFetcher {
    fun fetch(source: PriceSource): RateFetch
}

/** The price calls of the library, so that the fetching can be tested with no library. */
interface PricePort {
    fun sources(): List<PriceSource>

    fun agree(replies: List<PriceReply>, now: Long): AgreedPrice

    /** The sources of the shop's currency, three capital letters (OA-102). */
    fun sourcesFor(currency: String): List<PriceSource>

    /** The price the replies agree on in the shop's currency (OA-102). */
    fun agreeFor(currency: String, replies: List<PriceReply>, now: Long): AgreedPriceIn
}

/** The real thing: the library's own calls. */
object LibraryPrice : PricePort {
    override fun sources(): List<PriceSource> = OwnerPrice.sources()

    override fun agree(replies: List<PriceReply>, now: Long): AgreedPrice = OwnerPrice.agree(replies, now)

    override fun sourcesFor(currency: String): List<PriceSource> = OwnerPrice.sourcesFor(currency)

    override fun agreeFor(currency: String, replies: List<PriceReply>, now: Long): AgreedPriceIn =
        OwnerPrice.agreeFor(currency, replies, now)
}

/** What asking the sources came to. */
sealed class PriceOutcome {
    /** The sources agree: the price in whole dollars. */
    class Fetched(val usd: Long) : PriceOutcome()

    /** The sources of the shop's currency agree: the price in whole units of that currency (OA-102). */
    class FetchedIn(val whole: Long) : PriceOutcome()

    /** The sources did not answer or did not agree (the library's refusal). The owner types the price. */
    object Missing : PriceOutcome()
}

/**
 * Asking the price sources for the price of a bitcoin (OA-101). The sources, the reading of their replies and the
 * agreement between them are the library's; this decides only what is asked, and when each reply arrived.
 * No Android class is used.
 */
object PriceModel {
    /**
     * Asks every source of the library's table at once, each in a thread of its own, stamps each reply with
     * [clockSecs] when it has arrived whole, and hands the replies to the library. A source that gave no reply
     * has none in the call (RT-44). When the library refuses, or the library is missing, the owner types the price.
     */
    fun fetch(port: PricePort, fetcher: RateFetcher, clockSecs: () -> Long): PriceOutcome {
        val sources = try {
            port.sources()
        } catch (_: OwnerException) {
            return PriceOutcome.Missing
        }
        val replies = ask(sources, fetcher, clockSecs)
        return try {
            PriceOutcome.Fetched(port.agree(replies, clockSecs()).usdPerBtc)
        } catch (_: OwnerException) {
            PriceOutcome.Missing
        }
    }

    /**
     * The same for the shop's currency (OA-102): the sources of [currency] (three capital letters, not Bitcoin
     * Standard, which asks nothing), and the price in whole units of it.
     */
    fun fetchIn(currency: String, port: PricePort, fetcher: RateFetcher, clockSecs: () -> Long): PriceOutcome {
        val sources = try {
            port.sourcesFor(currency)
        } catch (_: OwnerException) {
            return PriceOutcome.Missing
        }
        val replies = ask(sources, fetcher, clockSecs)
        return try {
            PriceOutcome.FetchedIn(port.agreeFor(currency, replies, clockSecs()).wholePerBtc)
        } catch (_: OwnerException) {
            PriceOutcome.Missing
        }
    }

    private fun ask(sources: List<PriceSource>, fetcher: RateFetcher, clockSecs: () -> Long): List<PriceReply> {
        val replies = java.util.Collections.synchronizedList(ArrayList<PriceReply>())
        val threads = sources.map { source ->
            Thread {
                val got = try {
                    fetcher.fetch(source)
                } catch (_: RuntimeException) {
                    RateFetch.NotAnswered
                }
                if (got is RateFetch.Body) replies.add(PriceReply(source.id, got.bytes, clockSecs()))
            }.also { it.isDaemon = true; it.start() }
        }
        for (t in threads) t.join()
        return replies.toList()
    }

    /** Whole dollars with thousands separators, like 98,500. */
    fun dollars(price: Long): String = price.toString().reversed().chunked(3).joinToString(",").reversed()

    /** What the price field holds when the screen has the outcome: the fetched price, or nothing. */
    fun fieldText(outcome: PriceOutcome): String = when (outcome) {
        is PriceOutcome.Fetched -> outcome.usd.toString()
        is PriceOutcome.FetchedIn -> outcome.whole.toString()
        PriceOutcome.Missing -> ""
    }
}
