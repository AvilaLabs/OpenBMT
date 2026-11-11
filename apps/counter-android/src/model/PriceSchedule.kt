package org.openbmt.counter.app

import org.openbmt.counter.CounterException
import org.openbmt.counter.Rate
import org.openbmt.counter.RateSource
import org.openbmt.counter.Sample

/** The numbers of docs/RATE-RULES.md, as the app reads them. A test of the Rust crate checks that they are the same. */
object PriceLimits {
    /** RT-40: the seconds between two fetches of one source. */
    const val FETCH_INTERVAL_SECS = 60L

    /** RT-43: the seconds after which a fetch is given up. */
    const val FETCH_TIMEOUT_SECS = 20L

    /** RT-10: the most bytes of a reply that are read. */
    const val MAX_RESPONSE_BYTES = 64 * 1024
}

/** What one fetch gave. A reply, whole and read, or why there is none (RT-44). */
sealed class FetchResult {
    class Body(val bytes: ByteArray) : FetchResult()

    /** No reply: a timeout, a connection that failed. */
    class NotReached(val problem: String?) : FetchResult()

    /** A status other than 200. */
    class Status(val code: Int) : FetchResult()

    /** More than [PriceLimits.MAX_RESPONSE_BYTES] arrived. */
    object TooLong : FetchResult()

    /** Tor is on and the proxy did not answer: nothing was sent (CR-87). */
    object TorMissing : FetchResult()
}

/** Fetches one source's reply. The Android one is `HttpsFetcher`; the tests use a fake. */
fun interface Fetcher {
    fun fetch(source: RateSource): FetchResult
}

/** The price ops of the library, so that the schedule can be tested with no library. */
interface RatesPort {
    fun parse(source: String, body: ByteArray, fetchedAt: Long): Sample

    fun combine(samples: List<Sample>, now: Long): Rate
}

/** The real thing: the library's own ops. */
object LibraryRates : RatesPort {
    override fun parse(source: String, body: ByteArray, fetchedAt: Long): Sample =
        org.openbmt.counter.Rates.parse(source, body, fetchedAt)

    override fun combine(samples: List<Sample>, now: Long): Rate = org.openbmt.counter.Rates.combine(samples, now)
}

/**
 * RT-40 to RT-41: when each source is fetched. Every source has a fixed
 * turn, [intervalMs] after the last one's turn and not after the fetch
 * finished; a turn that was missed (the phone slept, a fetch ran long) is
 * skipped, never made up, so two fetches of a source are never closer than
 * one interval. Nothing here knows of sales: no sale, no requote and no key
 * press moves a turn (RT-41).
 */
class PriceSchedule(private val ids: List<String>, startMs: Long, val intervalMs: Long = PriceLimits.FETCH_INTERVAL_SECS * 1000) {
    private val next = HashMap<String, Long>().also { m -> ids.forEach { m[it] = startMs } }

    /** The milliseconds until the source's turn; 0 or less means it is due. */
    fun waitMs(id: String, nowMs: Long): Long = (next[id] ?: Long.MAX_VALUE) - nowMs

    fun due(id: String, nowMs: Long): Boolean = waitMs(id, nowMs) <= 0

    /** The turn of [id] was taken at [nowMs]: the next one is on the fixed grid, after now. */
    fun taken(id: String, nowMs: Long) {
        var n = next[id] ?: return
        while (n <= nowMs) n += intervalMs
        next[id] = n
    }
}

/** What the app knows of one source's last turn, for the owner's notes (RT-44). */
class SourceNote(val source: String, val atSecs: Long, val words: org.openbmt.app.SayWords?, val price: Long?)

/** The price status the home screen shows. */
sealed class PriceStatus {
    class Ready(val rate: Rate, val sources: Int) : PriceStatus()

    /** The library refused to make a rate; the words are its own. */
    class Refused(val words: org.openbmt.app.SayWords) : PriceStatus()

    object TorMissing : PriceStatus()
}

/**
 * The samples the schedule has collected (RT-41, RT-45): at most the newest of
 * each source, kept apart from the notes of the turns that failed. A sale is
 * quoted at [rate], which is the library's `combine` of what is here, never
 * of a fetch made for the sale.
 */
class PriceBook(private val rates: RatesPort, private val clockSecs: () -> Long) {
    private val samples = LinkedHashMap<String, Sample>()
    private val notes = LinkedHashMap<String, SourceNote>()
    private var torMissing = false

    /** The last turn of each source, for the owner. */
    fun notes(): List<SourceNote> = synchronized(this) { notes.values.toList() }

    /**
     * RT-44, RT-45: one turn's result. A reply is stamped now, when it has been
     * read whole, and made into a sample only if the library accepts it; any
     * other result is a missing sample, and nothing is kept from a refused
     * reply except the note.
     */
    fun record(source: String, result: FetchResult) {
        val now = clockSecs()
        val note: SourceNote = when (result) {
            is FetchResult.Body -> {
                torMissing = false
                try {
                    val s = rates.parse(source, result.bytes, now)
                    synchronized(this) { samples[source] = s }
                    SourceNote(source, now, null, s.minorPerBtc)
                } catch (e: CounterException) {
                    SourceNote(source, now, e.sayWords(), null)
                }
            }
            is FetchResult.NotReached -> { torMissing = false; SourceNote(source, now, AppWords.sourceNotAnswered, null) }
            is FetchResult.Status -> { torMissing = false; SourceNote(source, now, AppWords.sourceRefusedStatus(result.code), null) }
            FetchResult.TooLong -> { torMissing = false; SourceNote(source, now, AppWords.sourceReplyTooLong, null) }
            FetchResult.TorMissing -> { torMissing = true; SourceNote(source, now, AppWords.orbotNotRunning, null) }
        }
        synchronized(this) { notes[source] = note }
    }

    /**
     * The rate and its source now, or the library's refusal with its words. The
     * library judges freshness: a sample older than CR-58 allows is not used.
     */
    fun rate(): Result<Rate> {
        val snapshot = synchronized(this) { samples.values.toList() }
        return try {
            Result.success(rates.combine(snapshot, clockSecs()))
        } catch (e: CounterException) {
            Result.failure(e)
        }
    }

    /** What the home screen shows about the price. */
    fun status(): PriceStatus {
        if (torMissing) return PriceStatus.TorMissing
        val r = rate()
        val e = r.exceptionOrNull()
        if (e is CounterException) return PriceStatus.Refused(e.sayWords())
        val rate = r.getOrNull() ?: return PriceStatus.Refused(AppWords.fault(CounterException("rate-none", "no rate and no error", 4)))
        return PriceStatus.Ready(rate, rate.sourceLabel.removePrefix("med.").length)
    }
}

/**
 * One turn of one source: fetch, stamp, hand the bytes to the library. The
 * android side calls [turn] from one thread for each source when
 * [PriceSchedule.due] says so.
 */
class PriceRunner(private val book: PriceBook, private val fetcher: Fetcher) {
    fun turn(source: RateSource) {
        book.record(source.id, fetcher.fetch(source))
    }
}
