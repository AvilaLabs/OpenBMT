package org.openbmt.counter.app

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.Socket

/** What one request to the swap provider came to: the bytes it sent back, or that it sent none (CR-225). */
sealed class SwapOutcome {
    class Answer(val bytes: ByteArray) : SwapOutcome()

    object Failed : SwapOutcome()
}

/** The limits of CR-225. */
object SwapLimits {
    const val MAX_BYTES = 64 * 1024
    const val CONNECT_SECS = 60
    const val READ_SECS = 60
    const val PORT = 80
}

/**
 * CR-225: one request to the swap provider, as the library wrote it, plain HTTP/1.1 on port 80 of its
 * onion name, through the SOCKS proxy of the counter's own Tor at [proxy] with the name left to the proxy
 * to look up. Nothing is sent to a name that is not an onion name or to any other port, and when the
 * proxy does not answer, nothing is sent around it: there is no other way to reach the provider. The
 * answer is whatever the provider sends until it closes the connection, at most
 * [SwapLimits.MAX_BYTES] bytes; a connect or a read that does not end in 60 seconds fails.
 *
 * Not thread-safe by design: the app calls it from its one Lightning thread, one request at a time.
 */
class SwapTransport(private val proxy: String?) {
    fun send(hostPort: String, request: ByteArray): SwapOutcome {
        val hp = HostPort.split(hostPort) ?: return SwapOutcome.Failed
        if (!hp.first.lowercase().endsWith(".onion") || hp.second != SwapLimits.PORT) return SwapOutcome.Failed
        val p = proxy?.let { HostPort.split(it) } ?: return SwapOutcome.Failed
        val socket = Socket(Proxy(Proxy.Type.SOCKS, InetSocketAddress(p.first, p.second)))
        try {
            socket.soTimeout = SwapLimits.READ_SECS * 1000
            socket.connect(InetSocketAddress.createUnresolved(hp.first, hp.second), SwapLimits.CONNECT_SECS * 1000)
            socket.getOutputStream().apply {
                write(request)
                flush()
            }
            val out = ByteArrayOutputStream()
            val buf = ByteArray(4096)
            val input = socket.getInputStream()
            while (true) {
                val room = SwapLimits.MAX_BYTES + 1 - out.size()
                if (room <= 0) return SwapOutcome.Failed
                val n = input.read(buf, 0, minOf(buf.size, room))
                if (n < 0) break
                out.write(buf, 0, n)
            }
            return SwapOutcome.Answer(out.toByteArray())
        } catch (_: IOException) {
            return SwapOutcome.Failed
        } catch (_: SecurityException) {
            return SwapOutcome.Failed
        } finally {
            try {
                socket.close()
            } catch (_: IOException) {
            }
        }
    }
}
