package org.openbmt.checkout.square

import android.os.Handler
import android.os.Looper
import org.openbmt.checkout.CardResult
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors

/** Read-only Sandbox ListPayments lookup. An empty or ambiguous result stays UNKNOWN. */
internal class SquareSandboxReconciliation(
    private val credentials: SquareSandboxCredentials,
) {
    data class Outcome(val result: CardResult, val evidenceId: String, val amountMinor: Long)

    fun query(referenceId: String, amountMinor: Long, currency: String, complete: (Outcome?) -> Unit) {
        worker.execute {
            val found = runCatching { findExact(referenceId, amountMinor, currency) }.getOrNull()
            main.post { complete(found) }
        }
    }

    private fun findExact(referenceId: String, amountMinor: Long, currency: String): Outcome? {
        var cursor: String? = null
        val matches = mutableListOf<Outcome>()
        val seenCursors = mutableSetOf<String>()
        for (page in 1..MAX_PAGES) {
            val query = buildString {
                append("location_id=").append(enc(credentials.locationId))
                append("&total=").append(amountMinor)
                append("&limit=100")
                cursor?.let { append("&cursor=").append(enc(it)) }
            }
            val response = get("https://connect.squareupsandbox.com/v2/payments?$query")
            val payments = response.optJSONArray("payments")
            if (payments != null) for (index in 0 until payments.length()) {
                val payment = payments.optJSONObject(index) ?: continue
                val total = payment.optJSONObject("total_money") ?: continue
                if (payment.optString("reference_id") != referenceId ||
                    payment.optString("location_id") != credentials.locationId ||
                    payment.optString("source_type") != "CARD" ||
                    total.optLong("amount", Long.MIN_VALUE) != amountMinor ||
                    total.optString("currency") != currency
                ) continue
                val status = when (payment.optString("status")) {
                    "APPROVED" -> CardResult.APPROVED
                    "COMPLETED" -> CardResult.COMPLETED
                    "CANCELED" -> CardResult.CANCELED
                    "FAILED" -> CardResult.FAILED
                    else -> continue
                }
                val id = payment.optString("id").takeIf(String::isNotBlank) ?: continue
                matches += Outcome(status, id, total.optLong("amount"))
            }
            val nextCursor = response.optString("cursor").takeIf(String::isNotBlank)
            if (nextCursor == null) return matches.singleOrNull()
            if (!seenCursors.add(nextCursor)) return null
            if (page == MAX_PAGES) return null
            cursor = nextCursor
        }

        // A shared reference ID or multiple retries cannot identify one attempt safely.
        return null
    }

    private fun get(address: String): JSONObject {
        val connection = (URL(address).openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"
            instanceFollowRedirects = false
            connectTimeout = 10_000
            readTimeout = 15_000
            setRequestProperty("Authorization", "Bearer ${credentials.accessToken}")
            setRequestProperty("Square-Version", API_VERSION)
            setRequestProperty("Accept", "application/json")
        }
        try {
            val code = connection.responseCode
            val stream = if (code in 200..299) connection.inputStream else connection.errorStream
            val body = stream?.use { input ->
                val bytes = ByteArrayOutputStream()
                val buffer = ByteArray(8 * 1024)
                var total = 0
                while (true) {
                    val remaining = MAX_RESPONSE_BYTES + 1 - total
                    val count = input.read(buffer, 0, minOf(buffer.size, remaining))
                    if (count < 0) break
                    total += count
                    if (total > MAX_RESPONSE_BYTES) error("Square Sandbox response exceeded the size limit")
                    bytes.write(buffer, 0, count)
                }
                bytes.toString(Charsets.UTF_8.name())
            }.orEmpty()
            if (code !in 200..299) error("Square Sandbox lookup returned HTTP $code")
            return JSONObject(body)
        } finally {
            connection.disconnect()
        }
    }

    private fun enc(value: String): String = URLEncoder.encode(value, Charsets.UTF_8.name())

    private companion object {
        const val API_VERSION = "2026-09-16"
        const val MAX_PAGES = 20
        const val MAX_RESPONSE_BYTES = 1_048_576
        val worker = Executors.newSingleThreadExecutor()
        val main = Handler(Looper.getMainLooper())
    }
}
