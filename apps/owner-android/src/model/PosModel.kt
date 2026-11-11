package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import java.io.InputStream
import org.openbmt.app.SayWords
import org.openbmt.app.Sealer
import org.openbmt.app.SealedFile
import org.openbmt.owner.OwnerException
import org.openbmt.owner.PosReply
import org.openbmt.owner.PosRequest
import org.openbmt.owner.PosSetup
import org.openbmt.owner.PosState
import org.openbmt.owner.PosVendor

// The pure parts of the till screen (OA-16 to OA-19): the token's shape and
// its sealed file, what a typed setup becomes, which addresses may be read,
// the cap on a page, the pace of the reads and the loop of one read. The
// platform's connection is in `HttpsPosFetcher.kt`; no Android class is used here.

/** The limits of a read of the till. */
object PosLimits {
    /** The most bytes taken from the till's system in one page (OA-17). */
    const val MAX_PAGE_BYTES = 8 * 1024 * 1024

    /** The fewest milliseconds between the starts of two reads (OA-18). */
    const val MIN_POLL_MS = 60_000L

    /** The time to connect, and the time of each read of the answer (OA-17). */
    const val TIMEOUT_MS = 20_000

    /** The time of a whole page, whatever the connection does (OA-17). */
    const val DEADLINE_MS = 60_000L

    /** The most characters of a token (OA-16). */
    const val MAX_TOKEN_CHARS = 512

    /** The systems the app may read, and no other (OA-17). */
    val HOSTS: Set<String> = setOf("connect.squareup.com", "api.clover.com")

    /** A read is at most this many requests: the node's page limit and one more. */
    const val MAX_STEPS = 65
}

/** The shape of the read-only token the owner pastes (OA-16). */
object PosToken {
    /** The token as it is used, or null when it is not the shape of one: 1 to 512 printable characters and no space, so that it is one header value. */
    fun clean(text: String): String? {
        val t = text.trim()
        if (t.isEmpty() || t.length > PosLimits.MAX_TOKEN_CHARS) return null
        return t.takeIf { s -> s.all { it.code in 0x21..0x7e } }
    }
}

/**
 * The token, sealed in its own file of the app's `noBackupFilesDir` under its
 * own Keystore key (OA-16). It is written once, read only to make a request,
 * and removed with the till. Nothing here shows it.
 */
class PosTokenStore(dir: File, sealer: Sealer, name: String = AppFiles.POS_TOKEN_FILE) {
    private val file = File(dir, name)
    private val sealed = SealedFile(dir, sealer, name)

    fun exists(): Boolean = file.exists()

    fun save(token: String) = sealed.save(token.toByteArray(Charsets.UTF_8))

    /** The token, or null when none is kept. A file that does not read or does not open throws. */
    fun load(): String? = sealed.load()?.let { String(it, Charsets.UTF_8) }

    fun remove() {
        if (file.exists() && !file.delete()) throw IOException("the token file could not be removed")
    }
}

/** What the till screen turns typed text into. The library judges what it means (OA-07). */
object PosForms {
    fun token(text: String): Form<String> =
        PosToken.clean(text)?.let { Form.Ok(it) } ?: Form.Refused(AppWords.posTokenNotAccepted)

    /** Names or codes typed one per line or with commas, trimmed, with the empty ones left out. */
    fun list(text: String, spaces: Boolean = false): List<String> =
        text.split(if (spaces) Regex("[,\\s]+") else Regex("[,\\n]")).map { it.trim() }.filter { it.isNotEmpty() }

    fun setup(vendor: Int, location: String, tenders: String, currencies: String): PosSetup =
        PosSetup(
            if (vendor == 1) PosVendor.Clover else PosVendor.Square,
            location.trim(),
            list(tenders),
            list(currencies, spaces = true),
        )
}

/** The addresses the app sends a request to (OA-17). */
object PosUrl {
    /** True for `https://` and a host of [PosLimits.HOSTS] and a path, with no port, no login and nothing a header or a line could be hidden in. */
    fun allowed(url: String): Boolean {
        val rest = url.removePrefix("https://")
        if (rest.length == url.length) return false
        if (url.any { it.code <= 0x20 || it.code >= 0x7f }) return false
        val host = rest.substringBefore('/').substringBefore('?')
        if (host !in PosLimits.HOSTS) return false
        return rest.length > host.length && rest[host.length] == '/'
    }
}

/** At most [limit] bytes of [input]; null when there are more, so that the page is refused whole. Throws when [expired] says the time is up. */
fun readCapped(input: InputStream, limit: Int, expired: () -> Boolean = { false }): ByteArray? {
    val out = java.io.ByteArrayOutputStream()
    val buf = ByteArray(8192)
    while (true) {
        if (expired()) throw IOException("the page took longer than the time allowed")
        val room = limit + 1 - out.size()
        if (room <= 0) return null
        val n = input.read(buf, 0, minOf(buf.size, room))
        if (n < 0) break
        out.write(buf, 0, n)
    }
    return if (out.size() > limit) null else out.toByteArray()
}

/** What the till's system gave back to one request. */
sealed class PosFetch {
    /** An answer: the status and the body, whatever the status. */
    class Reply(val status: Long, val body: ByteArray) : PosFetch()

    /** Orbot did not answer, so nothing was sent. */
    object TorMissing : PosFetch()

    /** The body was longer than a page may be. */
    object TooLong : PosFetch()

    /** No answer came, with the kind of the problem. */
    class NotReached(val why: String) : PosFetch()
}

/** Sends one request of the library to the till's system with the sign-in the app holds. */
fun interface PosFetcher {
    fun fetch(request: PosRequest, token: String): PosFetch
}

/** The two calls of a read, so that its loop can be run without the library. */
interface PosLink {
    fun request(now: Long): PosRequest

    fun reply(now: Long, status: Long, body: ByteArray): PosReply
}

/** What a read did. */
sealed class PosOutcome {
    class Done(val pages: Long, val ordersRead: Long, val newAlerts: Int, val moreLeft: Boolean) : PosOutcome()

    class Failed(val words: SayWords) : PosOutcome()
}

object PosPoll {
    /**
     * One read of the till: the library's request, sent by [fetcher], its answer handed back, until the library
     * wants no more pages (OA-17). Anything that goes wrong ends the read with words; nothing is retried here.
     */
    fun readOnce(link: PosLink, fetcher: PosFetcher, token: String, clock: () -> Long): PosOutcome {
        var newAlerts = 0
        for (step in 1..PosLimits.MAX_STEPS) {
            val request = try {
                link.request(clock())
            } catch (e: OwnerException) {
                return PosOutcome.Failed(e.sayWords())
            }
            if (!PosUrl.allowed(request.url)) return PosOutcome.Failed(AppWords.posAddressRefused)
            when (val f = fetcher.fetch(request, token)) {
                is PosFetch.TorMissing -> return PosOutcome.Failed(AppWords.orbotNotRunning)
                is PosFetch.TooLong -> return PosOutcome.Failed(AppWords.posPageTooLong)
                is PosFetch.NotReached -> return PosOutcome.Failed(AppWords.posNotReached(f.why))
                is PosFetch.Reply -> {
                    val r = try {
                        link.reply(clock(), f.status, f.body)
                    } catch (e: OwnerException) {
                        return PosOutcome.Failed(e.sayWords())
                    }
                    newAlerts += r.newAlerts.size
                    if (!r.again) return PosOutcome.Done(r.pages, r.ordersRead, newAlerts, r.moreLeft)
                }
            }
        }
        return PosOutcome.Failed(AppWords.posReadCutOff)
    }
}

/** The pace of the reads (OA-18). */
object PosSchedule {
    /** Milliseconds until a read may start: 0 when one may, never more than a minute. */
    fun waitMs(lastStartMs: Long?, nowMs: Long): Long {
        if (lastStartMs == null) return 0
        val next = lastStartMs + PosLimits.MIN_POLL_MS
        return (next - nowMs).coerceIn(0, PosLimits.MIN_POLL_MS)
    }
}

/** The lines around the library's words on the till screen. */
object PosLines {
    fun lastRead(now: Long, at: Long?): String =
        if (at == null) "Not read yet since the app was opened." else "Last read ${OwnerModel.ago(now, at)}."

    fun counts(s: PosState): String {
        val byOrder = if (s.matchesByOrder > 0) " ${s.matchesByOrder} of them by order of time, not exactly." else ""
        val matches = if (s.matches == 1L) "1 sale matched." else "${s.matches} sales matched."
        return "$matches$byOrder ${OwnerModel.alertsLine(s.openAlerts.size)}"
    }

    fun till(setup: PosSetup): String =
        "${if (setup.vendor == PosVendor.Clover) "Clover" else "Square"}, ${setup.location}. " +
            "Bitcoin tender: ${setup.tenderNames.joinToString(", ")}. Currencies: ${setup.currencies.joinToString(", ")}."
}
