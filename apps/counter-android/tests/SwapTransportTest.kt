package org.openbmt.counter.app

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket

/**
 * The transport of CR-225 against a local SOCKS proxy that forwards to a
 * local plain-HTTP server, on the desktop JVM: an onion name on port 80 goes
 * to the proxy as a name, unresolved, and the bytes of the request reach the
 * server as they are; the answer is everything until the server closes, at
 * most 64 KiB; any other name or port, a missing proxy and a proxy that does
 * not answer send nothing, and nothing goes around the proxy.
 */
object SwapTransportTest {
    private var failures = 0
    private var checks = 0

    private fun ok(cond: Boolean, what: String) {
        checks++
        if (!cond) {
            failures++
            println("FAIL: $what")
        }
    }

    /** A server that reads one request up to the blank line, answers with [reply] and closes. */
    private class Server(private val reply: ByteArray) {
        val socket = ServerSocket(0, 5, InetAddress.getByName("127.0.0.1"))
        val got = java.util.concurrent.CopyOnWriteArrayList<ByteArray>()
        val port: Int get() = socket.localPort

        init {
            Thread {
                while (!socket.isClosed) {
                    try {
                        val s = socket.accept()
                        s.soTimeout = 3000
                        val head = readHead(s.getInputStream())
                        got.add(head)
                        s.getOutputStream().apply {
                            write(reply)
                            flush()
                        }
                        s.close()
                    } catch (_: java.io.IOException) {
                    }
                }
            }.also { it.isDaemon = true; it.start() }
        }

        private fun readHead(input: InputStream): ByteArray {
            val out = ByteArrayOutputStream()
            while (!out.toString(Charsets.ISO_8859_1).endsWith("\r\n\r\n")) {
                val b = input.read()
                if (b < 0) break
                out.write(b)
            }
            return out.toByteArray()
        }

        fun close() = socket.close()
    }

    /** A SOCKS5 proxy with no authentication that sends every name to the local server at [target]. */
    private class Socks(private val target: Int?) {
        val socket = ServerSocket(0, 5, InetAddress.getByName("127.0.0.1"))
        val asked = java.util.concurrent.CopyOnWriteArrayList<String>()
        val ports = java.util.concurrent.CopyOnWriteArrayList<Int>()
        val addressTypes = java.util.concurrent.CopyOnWriteArrayList<Int>()
        val port: Int get() = socket.localPort

        init {
            Thread {
                while (!socket.isClosed) {
                    try {
                        val s = socket.accept()
                        Thread { serve(s) }.also { it.isDaemon = true; it.start() }
                    } catch (_: java.io.IOException) {
                    }
                }
            }.also { it.isDaemon = true; it.start() }
        }

        private fun serve(s: Socket) {
            try {
                s.soTimeout = 3000
                val i = s.getInputStream()
                val o = s.getOutputStream()
                i.read() // version
                val methods = i.read()
                repeat(methods) { i.read() }
                o.write(byteArrayOf(5, 0))
                i.read(); i.read(); i.read() // version, command, reserved
                val type = i.read()
                addressTypes.add(type)
                if (type == 3) {
                    val len = i.read()
                    val name = ByteArray(len)
                    var at = 0
                    while (at < len) at += i.read(name, at, len - at)
                    asked.add(String(name))
                }
                ports.add((i.read() shl 8) or i.read())
                val to = target
                if (to == null) {
                    o.write(byteArrayOf(5, 5, 0, 1, 0, 0, 0, 0, 0, 0))
                    s.close()
                    return
                }
                val up = Socket(InetAddress.getByName("127.0.0.1"), to)
                o.write(byteArrayOf(5, 0, 0, 1, 0, 0, 0, 0, 0, 0))
                o.flush()
                val pump = { from: InputStream, into: java.io.OutputStream ->
                    try {
                        val buf = ByteArray(4096)
                        while (true) {
                            val n = from.read(buf)
                            if (n < 0) break
                            into.write(buf, 0, n)
                            into.flush()
                        }
                    } catch (_: java.io.IOException) {
                    }
                    try { into.close() } catch (_: java.io.IOException) { }
                }
                Thread { pump(s.getInputStream(), up.getOutputStream()) }.also { it.isDaemon = true; it.start() }
                pump(up.getInputStream(), s.getOutputStream())
            } catch (_: java.io.IOException) {
            }
        }

        fun close() = socket.close()
    }

    private val request = "GET /api/v2/swap/reverse HTTP/1.1\r\nHost: abcdefghijklmnop.onion\r\nConnection: close\r\n\r\n".toByteArray()
    private val reply = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".toByteArray()

    @JvmStatic
    fun main(args: Array<String>) {
        // An onion name on port 80 goes to the proxy as a name, and the request reaches the server as it is.
        run {
            val server = Server(reply)
            val socks = Socks(server.port)
            val t = SwapTransport("127.0.0.1:${socks.port}")
            val out = t.send("abcdefghijklmnop.onion:80", request)
            ok(out is SwapOutcome.Answer && out.bytes.contentEquals(reply), "the answer is what the server sent")
            ok(server.got.singleOrNull()?.contentEquals(request) == true, "the request reached the server byte for byte")
            ok(socks.asked.toList() == listOf("abcdefghijklmnop.onion") && socks.addressTypes.toList() == listOf(3), "the name went to the proxy unresolved: ${socks.asked} ${socks.addressTypes}")
            ok(socks.ports.toList() == listOf(80), "port 80: ${socks.ports}")
            socks.close()
            server.close()
        }

        // Nothing is sent to a name that is not an onion name, or to another port.
        run {
            val server = Server(reply)
            val socks = Socks(server.port)
            val t = SwapTransport("127.0.0.1:${socks.port}")
            ok(t.send("provider.example:80", request) is SwapOutcome.Failed, "a clearnet name is refused")
            ok(t.send("127.0.0.1:${server.port}", request) is SwapOutcome.Failed, "a direct address is refused")
            ok(t.send("abcdefghijklmnop.onion:443", request) is SwapOutcome.Failed, "another port is refused")
            ok(t.send("no port", request) is SwapOutcome.Failed, "an address that is not host:port is refused")
            ok(socks.asked.isEmpty() && server.got.isEmpty(), "the proxy was not asked and the server saw nothing")
            socks.close()
            server.close()
        }

        // With no proxy address, or a proxy that is down or says no, nothing goes around it.
        run {
            val server = Server(reply)
            ok(SwapTransport(null).send("abcdefghijklmnop.onion:80", request) is SwapOutcome.Failed, "no proxy address: nothing is sent")
            val down = ServerSocket(0).use { it.localPort }
            ok(SwapTransport("127.0.0.1:$down").send("abcdefghijklmnop.onion:80", request) is SwapOutcome.Failed, "the proxy is down: nothing is sent")
            val no = Socks(null)
            ok(SwapTransport("127.0.0.1:${no.port}").send("abcdefghijklmnop.onion:80", request) is SwapOutcome.Failed, "the proxy said no")
            ok(server.got.isEmpty(), "nothing went around the proxy")
            no.close()
            server.close()
        }

        // The answer is at most 64 KiB.
        run {
            val exact = ByteArray(SwapLimits.MAX_BYTES) { 'a'.code.toByte() }
            val atLimit = Server(exact)
            val socksA = Socks(atLimit.port)
            val a = SwapTransport("127.0.0.1:${socksA.port}").send("abcdefghijklmnop.onion:80", request)
            ok(a is SwapOutcome.Answer && a.bytes.size == SwapLimits.MAX_BYTES, "an answer at the limit is taken")
            val big = Server(ByteArray(SwapLimits.MAX_BYTES + 1) { 'a'.code.toByte() })
            val socksB = Socks(big.port)
            val b = SwapTransport("127.0.0.1:${socksB.port}").send("abcdefghijklmnop.onion:80", request)
            ok(b is SwapOutcome.Failed, "an answer past the limit is refused")
            socksA.close(); socksB.close(); atLimit.close(); big.close()
        }

        println("SwapTransportTest: $checks checks, $failures failures")
        if (failures > 0) System.exit(1)
    }
}
