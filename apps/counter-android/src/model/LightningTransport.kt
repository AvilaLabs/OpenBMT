package org.openbmt.counter.app

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.Socket
import java.net.SocketTimeoutException
import java.net.URI
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLEngine
import javax.net.ssl.TrustManager
import javax.net.ssl.X509ExtendedTrustManager
import org.openbmt.counter.LnMethod
import org.openbmt.counter.LnRequest
import org.openbmt.counter.LnTransport

/** What one request to the shop's node came to: its answer, or why it got none (CR-114). */
sealed class LnOutcome {
    class Answer(val status: Int, val body: ByteArray) : LnOutcome()

    class Failed(val why: LnTransport) : LnOutcome()
}

/** The limits of CR-114. */
object LightningLimits {
    const val MAX_BODY_BYTES = 1_000_000
    const val CONNECT_SECS_TOR = 20
    const val CONNECT_SECS_DIRECT = 10
    const val READ_SECS = 15
}

/**
 * CR-114 on the platform's `HttpsURLConnection`: one request to the shop's
 * node, HTTPS only, no redirect, no cookies, no cache, no compression, a body
 * of at most [LightningLimits.MAX_BODY_BYTES] bytes, a connect timeout of 20
 * seconds through Tor and 10 directly, and a read timeout of 15.
 *
 * The connection goes through the SOCKS proxy at [proxy] with the name of the
 * node left to the proxy to look up when the host ends in `.onion` or the
 * shop's settings have Tor on ([torOn]); if the proxy does not answer, nothing
 * is sent around it. Any other host is reached directly, a home-network
 * address included, and the proxy of the system is not used either.
 *
 * The certificate is not checked against any authority and the host name is
 * not checked at all: the one certificate the node showed goes to [pin]
 * (the library's `ln_pin`) as DER, and unless it says yes the handshake ends
 * before a byte of the request is written.
 *
 * Not thread-safe by design: the app calls it from its one Lightning thread.
 */
class LightningTransport(private val torOn: Boolean, private val proxy: String?) {
    fun send(request: LnRequest, pin: (ByteArray) -> Boolean): LnOutcome {
        val hp = HostPort.split(request.hostPort) ?: return LnOutcome.Failed(LnTransport.Unreachable)
        val viaTor = torOn || hp.first.lowercase().endsWith(".onion")
        val socks = if (viaTor) {
            val p = proxy?.let { HostPort.split(it) } ?: return LnOutcome.Failed(LnTransport.Unreachable)
            Proxy(Proxy.Type.SOCKS, InetSocketAddress(p.first, p.second))
        } else {
            Proxy.NO_PROXY
        }
        val refused = booleanArrayOf(false)
        val trust = PinnedTrust(pin, refused)
        var conn: HttpsURLConnection? = null
        try {
            val ssl = SSLContext.getInstance("TLS")
            ssl.init(null, arrayOf<TrustManager>(trust), SecureRandom())
            val url = URI("https://" + request.hostPort + request.path).toURL()
            conn = url.openConnection(socks) as HttpsURLConnection
            conn.sslSocketFactory = ssl.socketFactory
            // The pin is the check; the name is not one (CR-114).
            conn.hostnameVerifier = HostnameVerifier { _, _ -> true }
            conn.requestMethod = if (request.method == LnMethod.Post) "POST" else "GET"
            conn.useCaches = false
            conn.defaultUseCaches = false
            conn.instanceFollowRedirects = false
            conn.connectTimeout = (if (viaTor) LightningLimits.CONNECT_SECS_TOR else LightningLimits.CONNECT_SECS_DIRECT) * 1000
            conn.readTimeout = LightningLimits.READ_SECS * 1000
            conn.setRequestProperty("Accept", "application/json")
            conn.setRequestProperty("Accept-Encoding", "identity")
            conn.setRequestProperty(request.headerName, request.headerValue)
            val body = request.body
            if (body != null) {
                val bytes = body.toByteArray(Charsets.UTF_8)
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.outputStream.use { it.write(bytes) }
            }
            val code = conn.responseCode
            if (code in 300..399) return LnOutcome.Failed(LnTransport.Redirected)
            val declared = conn.contentLengthLong
            if (declared > LightningLimits.MAX_BODY_BYTES) return LnOutcome.Failed(LnTransport.TooBig)
            val stream = if (code >= 400) conn.errorStream else conn.inputStream
            val out = ByteArrayOutputStream()
            if (stream != null) {
                val buf = ByteArray(8192)
                stream.use { input ->
                    while (true) {
                        val room = LightningLimits.MAX_BODY_BYTES + 1 - out.size()
                        if (room <= 0) return LnOutcome.Failed(LnTransport.TooBig)
                        val n = input.read(buf, 0, minOf(buf.size, room))
                        if (n < 0) break
                        out.write(buf, 0, n)
                    }
                }
            }
            return LnOutcome.Answer(code, out.toByteArray())
        } catch (e: SocketTimeoutException) {
            return LnOutcome.Failed(if (refused[0]) LnTransport.PinRefused else LnTransport.TimedOut)
        } catch (e: IOException) {
            return LnOutcome.Failed(if (refused[0]) LnTransport.PinRefused else LnTransport.Unreachable)
        } catch (e: java.security.GeneralSecurityException) {
            return LnOutcome.Failed(LnTransport.Unreachable)
        } catch (e: SecurityException) {
            return LnOutcome.Failed(LnTransport.Unreachable)
        } finally {
            conn?.disconnect()
        }
    }
}

/**
 * The trust of one connection: the leaf the node shows, as DER, goes to [pin]
 * and nothing else is looked at. A no from [pin] is noted in [refused] and
 * ends the handshake. The extended class keeps the platform from adding the
 * host name and algorithm checks of its own around a plain trust manager.
 */
private class PinnedTrust(private val pin: (ByteArray) -> Boolean, private val refused: BooleanArray) : X509ExtendedTrustManager() {
    private fun check(chain: Array<out X509Certificate>?) {
        val leaf = chain?.firstOrNull() ?: throw CertificateException("the node showed no certificate")
        if (!pin(leaf.encoded)) {
            refused[0] = true
            throw CertificateException("the certificate is not the pinned one")
        }
    }

    override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) = check(chain)

    override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?, socket: Socket?) = check(chain)

    override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?, engine: SSLEngine?) = check(chain)

    // The node is never asked for a certificate of ours, and no client is trusted.
    override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) =
        throw CertificateException("no client is trusted")

    override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?, socket: Socket?) =
        throw CertificateException("no client is trusted")

    override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?, engine: SSLEngine?) =
        throw CertificateException("no client is trusted")

    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
}
