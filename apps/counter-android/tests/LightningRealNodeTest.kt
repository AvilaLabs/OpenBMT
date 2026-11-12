package org.openbmt.counter.app

import java.io.File
import org.openbmt.counter.LnMethod
import org.openbmt.counter.LnRequest

/**
 * The transport of CR-114 against real nodes, on the desktop JVM. Not part of
 * run-jvm.sh: `scripts/lightning-e2e-core.sh` starts a regtest LND and Core
 * Lightning, has the `lightning_regtest` example write the requests its
 * crate builds into a directory (`<name>.req`: the host and port, the method,
 * the path, the name and the value of the credential's header, the file of the
 * DER certificate to pin, then the body), runs this, and has the example read
 * the answers back (`<name>.status`: the HTTP status, or the name of why there
 * was none; `<name>.body`).
 */
object LightningRealNodeTest {
    @JvmStatic
    fun main(args: Array<String>) {
        val dir = File(args[0])
        val transport = LightningTransport(false, null)
        var sent = 0
        val requests = dir.listFiles { f -> f.name.endsWith(".req") }?.sortedBy { it.name } ?: emptyList()
        if (requests.isEmpty()) {
            println("LightningRealNodeTest: no requests in $dir")
            System.exit(1)
        }
        for (file in requests) {
            val lines = file.readText(Charsets.UTF_8).split("\n")
            val method = if (lines[1] == "POST") LnMethod.Post else LnMethod.Get
            val pinned = File(lines[5]).readBytes()
            val body = lines.drop(6).joinToString("\n").ifEmpty { null }
            val request = LnRequest(sent.toLong(), lines[0], method, lines[2], lines[3], lines[4], body)
            val name = file.name.removeSuffix(".req")
            when (val out = transport.send(request) { it.contentEquals(pinned) }) {
                is LnOutcome.Answer -> {
                    File(dir, "$name.status").writeText(out.status.toString())
                    File(dir, "$name.body").writeBytes(out.body)
                    println("$name: ${request.method} ${request.path} -> HTTP ${out.status}, ${out.body.size} bytes")
                }
                is LnOutcome.Failed -> {
                    File(dir, "$name.status").writeText(out.why.name)
                    println("$name: ${request.method} ${request.path} -> ${out.why}")
                }
            }
            sent++
        }
        println("LightningRealNodeTest: $sent requests sent")
    }
}
