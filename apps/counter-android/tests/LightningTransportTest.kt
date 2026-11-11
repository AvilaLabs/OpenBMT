package org.openbmt.counter.app

import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.net.ServerSocket
import java.net.Socket
import java.security.KeyStore
import javax.net.ssl.KeyManagerFactory
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLServerSocket
import javax.net.ssl.SSLSocket
import org.openbmt.counter.LnMethod
import org.openbmt.counter.LnRequest
import org.openbmt.counter.LnTransport

/**
 * The transport of CR-114 against a local HTTPS server with a self-signed
 * certificate (made by `keytool` in run-jvm.sh), on the desktop JVM: a pin
 * that matches lets the request go, a pin that differs ends the handshake
 * before one byte of the request is sent, a redirect and a body past the
 * limit are refused, the name of the host is not checked, and a `.onion`
 * name goes to the SOCKS proxy unresolved and never around it.
 */
object LightningTransportTest {
    private var failures = 0
    private var checks = 0

    private fun ok(cond: Boolean, what: String) {
        checks++
        if (!cond) {
            failures++
            println("FAIL: $what")
        }
    }

    /** What the server did with a connection. */
    private class Seen(val handshook: Boolean, val requestBytes: Int, val head: String)

    /** A server that answers every request with [reply] and notes what it got. */
    private class Server(private val ssl: SSLContext, private val reply: (Socket) -> Unit) {
        val socket = ssl.serverSocketFactory.createServerSocket(0, 5, java.net.InetAddress.getByName("127.0.0.1")) as SSLServerSocket
        val seen = java.util.concurrent.CopyOnWriteArrayList<Seen>()
        private val thread = Thread {
            while (!socket.isClosed) {
                try {
                    val s = socket.accept() as SSLSocket
                    handle(s)
                } catch (_: java.io.IOException) {
                }
            }
        }.also { it.isDaemon = true; it.start() }

        val port: Int get() = socket.localPort

        private fun handle(s: SSLSocket) {
            try {
                s.soTimeout = 3000
                try {
                    s.startHandshake()
                } catch (_: java.io.IOException) {
                    seen.add(Seen(false, 0, ""))
                    return
                }
                val head = ByteArrayOutputStream()
                val input = s.inputStream
                var total = 0
                val end = "\r\n\r\n".toByteArray()
                var matched = 0
                while (matched < end.size) {
                    val b = input.read()
                    if (b < 0) break
                    total++
                    head.write(b)
                    matched = if (b == end[matched].toInt()) matched + 1 else if (b == end[0].toInt()) 1 else 0
                }
                val text = head.toString(Charsets.UTF_8)
                val length = Regex("(?i)content-length: (\\d+)").find(text)?.groupValues?.get(1)?.toInt() ?: 0
                var left = length
                while (left > 0) {
                    val n = input.read(ByteArray(minOf(left, 4096)))
                    if (n < 0) break
                    total += n
                    left -= n
                }
                seen.add(Seen(true, total, text))
                reply(s)
            } catch (_: java.io.IOException) {
            } finally {
                try { s.close() } catch (_: java.io.IOException) { }
            }
        }

        fun close() = socket.close()
    }

    private fun request(port: Int, host: String = "127.0.0.1", method: LnMethod = LnMethod.Post, body: String? = "{\"a\":1}") =
        LnRequest(7, "$host:$port", method, "/v1/invoices", "Grpc-Metadata-macaroon", "00aa", body)

    private fun http(status: String, headers: String, body: ByteArray): (Socket) -> Unit = { s ->
        val out = s.getOutputStream()
        out.write("HTTP/1.1 $status\r\n$headers\r\nConnection: close\r\n\r\n".toByteArray())
        out.write(body)
        out.flush()
    }

    /** A SOCKS5 server that notes the name asked for and says no. */
    private class Socks {
        val socket = ServerSocket(0, 5, java.net.InetAddress.getByName("127.0.0.1"))
        val asked = java.util.concurrent.CopyOnWriteArrayList<String>()
        val addressTypes = java.util.concurrent.CopyOnWriteArrayList<Int>()
        private val thread = Thread {
            while (!socket.isClosed) {
                try {
                    socket.accept().use { s ->
                        s.soTimeout = 3000
                        val i: InputStream = s.getInputStream()
                        val greeting = i.readNBytes(2)
                        i.readNBytes(greeting[1].toInt())
                        s.getOutputStream().write(byteArrayOf(5, 0))
                        val head = i.readNBytes(4)
                        addressTypes.add(head[3].toInt())
                        if (head[3].toInt() == 3) {
                            val len = i.read()
                            asked.add(String(i.readNBytes(len), Charsets.US_ASCII))
                        }
                        s.getOutputStream().write(byteArrayOf(5, 5, 0, 1, 0, 0, 0, 0, 0, 0))
                    }
                } catch (_: java.io.IOException) {
                }
            }
        }.also { it.isDaemon = true; it.start() }

        val port: Int get() = socket.localPort

        fun close() = socket.close()
    }

    private fun sslContext(keystore: File): Pair<SSLContext, ByteArray> {
        val ks = KeyStore.getInstance("PKCS12")
        keystore.inputStream().use { ks.load(it, "changeit".toCharArray()) }
        val kmf = KeyManagerFactory.getInstance(KeyManagerFactory.getDefaultAlgorithm())
        kmf.init(ks, "changeit".toCharArray())
        val ctx = SSLContext.getInstance("TLS")
        ctx.init(kmf.keyManagers, null, null)
        return ctx to ks.getCertificate("t").encoded
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val (ssl, der) = sslContext(File(args[0]))
        val direct = LightningTransport(false, null)
        val other = ByteArray(der.size).also { it[0] = der[0] }

        // The pin matches: the request goes, with its header and body, and the answer comes back.
        run {
            val server = Server(ssl, http("200 OK", "Content-Type: application/json\r\nContent-Length: 11", "{\"ok\":true}".toByteArray()))
            val offered = ArrayList<ByteArray>()
            val out = direct.send(request(server.port)) { d -> offered.add(d); d.contentEquals(der) }
            ok(out is LnOutcome.Answer && out.status == 200 && String(out.body) == "{\"ok\":true}", "a matching pin: the answer comes back")
            ok(offered.size == 1 && offered[0].contentEquals(der), "the leaf of the node went to the pin, as DER")
            val s = server.seen.singleOrNull()
            ok(s != null && s.handshook && s.requestBytes > 0, "a matching pin: the request was sent")
            ok(s != null && s.head.startsWith("POST /v1/invoices HTTP/1.1"), "the method and path")
            ok(s != null && s.head.contains("Grpc-Metadata-macaroon: 00aa"), "the header of the credential")
            ok(s != null && !s.head.contains("Cookie", ignoreCase = true), "no cookie")
            server.close()
        }

        // A GET has no body; a name that is not the certificate's is not checked (the pin is the check).
        run {
            val server = Server(ssl, http("200 OK", "Content-Length: 2", "[]".toByteArray()))
            val out = direct.send(request(server.port, host = "localhost", method = LnMethod.Get, body = null)) { it.contentEquals(der) }
            ok(out is LnOutcome.Answer && out.status == 200, "a GET with the certificate's name is fine")
            ok(server.seen.singleOrNull()?.head?.startsWith("GET /v1/invoices HTTP/1.1") == true, "the GET line")
            val out2 = direct.send(request(server.port, host = "127.0.0.1")) { it.contentEquals(der) }
            ok(out2 is LnOutcome.Answer, "the host name is not checked against the certificate")
            server.close()
        }

        // The pin differs: refused, and the server never saw a byte of the request.
        run {
            val server = Server(ssl, http("200 OK", "Content-Length: 2", "{}".toByteArray()))
            val out = direct.send(request(server.port)) { it.contentEquals(other) }
            ok(out is LnOutcome.Failed && out.why == LnTransport.PinRefused, "a different pin is refused: $out")
            Thread.sleep(300)
            ok(server.seen.all { !it.handshook && it.requestBytes == 0 }, "a different pin: no request byte reached the server")
            server.close()
        }

        // An error status is an answer: the library reads what the node said.
        run {
            val server = Server(ssl, http("401 Unauthorized", "Content-Length: 5", "nope!".toByteArray()))
            val out = direct.send(request(server.port)) { it.contentEquals(der) }
            ok(out is LnOutcome.Answer && out.status == 401 && String(out.body) == "nope!", "a 401 comes back with its body")
            server.close()
        }

        // A redirect is refused and not followed.
        run {
            val target = Server(ssl, http("200 OK", "Content-Length: 2", "{}".toByteArray()))
            val server = Server(ssl, http("302 Found", "Location: https://127.0.0.1:${target.port}/x\r\nContent-Length: 0", ByteArray(0)))
            val out = direct.send(request(server.port)) { it.contentEquals(der) }
            ok(out is LnOutcome.Failed && out.why == LnTransport.Redirected, "a redirect is refused: $out")
            ok(target.seen.isEmpty(), "the redirect was not followed")
            target.close()
            server.close()
        }

        // A body past the limit is refused, declared or not; a body at the limit is taken.
        run {
            val big = ByteArray(LightningLimits.MAX_BODY_BYTES + 1) { 'a'.code.toByte() }
            val declared = Server(ssl, http("200 OK", "Content-Length: ${big.size}", big))
            val out = direct.send(request(declared.port)) { it.contentEquals(der) }
            ok(out is LnOutcome.Failed && out.why == LnTransport.TooBig, "a declared body past the limit is refused: $out")
            declared.close()
            val undeclared = Server(ssl, http("200 OK", "", big))
            val out2 = direct.send(request(undeclared.port)) { it.contentEquals(der) }
            ok(out2 is LnOutcome.Failed && out2.why == LnTransport.TooBig, "an undeclared body past the limit is refused: $out2")
            undeclared.close()
            val exact = ByteArray(LightningLimits.MAX_BODY_BYTES) { 'a'.code.toByte() }
            val atLimit = Server(ssl, http("200 OK", "Content-Length: ${exact.size}", exact))
            val out3 = direct.send(request(atLimit.port)) { it.contentEquals(der) }
            ok(out3 is LnOutcome.Answer && out3.body.size == LightningLimits.MAX_BODY_BYTES, "a body at the limit is taken")
            atLimit.close()
        }

        // Nothing listening is unreachable; a server that never answers times out is not tested here (15 s).
        run {
            val closed = ServerSocket(0).use { it.localPort }
            val out = direct.send(request(closed)) { true }
            ok(out is LnOutcome.Failed && out.why == LnTransport.Unreachable, "a closed port is unreachable: $out")
            val bad = direct.send(LnRequest(1, "no port", LnMethod.Get, "/", "h", "v", null)) { true }
            ok(bad is LnOutcome.Failed && bad.why == LnTransport.Unreachable, "an address that is not host:port is unreachable")
        }

        // Tor: the name goes to the proxy unresolved; with the proxy down nothing is sent around it.
        run {
            val server = Server(ssl, http("200 OK", "Content-Length: 2", "{}".toByteArray()))
            val socks = Socks()
            val viaOnion = LightningTransport(false, "127.0.0.1:${socks.port}")
            val out = viaOnion.send(request(server.port, host = "abcdefghijklmnop.onion")) { true }
            ok(out is LnOutcome.Failed && out.why == LnTransport.Unreachable, "the proxy said no: $out")
            ok(socks.asked.toList() == listOf("abcdefghijklmnop.onion") && socks.addressTypes.toList() == listOf(3), "a .onion name goes to the proxy as a name, unresolved: ${socks.asked} ${socks.addressTypes}")
            val torOn = LightningTransport(true, "127.0.0.1:${socks.port}")
            torOn.send(request(server.port, host = "node.example")) { true }
            ok(socks.asked.toList() == listOf("abcdefghijklmnop.onion", "node.example"), "with Tor on every host goes through the proxy: ${socks.asked}")
            ok(server.seen.isEmpty(), "nothing went around the proxy")
            val down = ServerSocket(0).use { it.localPort }
            val noProxy = LightningTransport(true, "127.0.0.1:$down").send(request(server.port)) { true }
            ok(noProxy is LnOutcome.Failed && noProxy.why == LnTransport.Unreachable && server.seen.isEmpty(), "Tor on and no proxy: nothing is sent, not even direct")
            val noAddress = LightningTransport(true, null).send(request(server.port)) { true }
            ok(noAddress is LnOutcome.Failed && server.seen.isEmpty(), "Tor on and no proxy address: nothing is sent")
            socks.close()
            server.close()
        }

        println("LightningTransportTest: $checks checks, $failures failures")
        if (failures > 0) System.exit(1)
    }
}
