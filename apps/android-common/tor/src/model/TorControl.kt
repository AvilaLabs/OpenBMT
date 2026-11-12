package org.openbmt.app

import java.io.BufferedInputStream
import java.io.IOException
import java.io.InputStream

// The little Tor says on its control socket, read strictly (OA-97). The app sends five commands and no other,
// and reads a reply only if it has the exact form of the control protocol: a three-digit code, a separator, text
// of printable ASCII, a carriage return and a line feed. Anything else ends the run with an error. No Android
// class is used here; the socket itself is in `TorService.kt`.

/** Tor said something this reader does not accept. */
class TorControlException(message: String) : IOException(message)

/** One reply or event: its code and the text of each of its lines, without the code, the separator or the line end. */
class ControlMessage(val code: Int, val lines: List<String>) {
    val isEvent: Boolean get() = code == TorControl.CODE_EVENT
}

/**
 * Reads messages from the control socket. A line is at most [TorControl.MAX_LINE] bytes before its line end, a
 * message at most [TorControl.MAX_LINES] lines; the line end is a carriage return and a line feed, never either
 * alone; every byte is printable ASCII; a message ends with a line whose separator is a space, and a stream that
 * ends before that is refused. A data reply (separator `+`) is refused: none of the five commands asks for one.
 */
class ControlReader(input: InputStream) {
    private val input = BufferedInputStream(input)

    /** The next message, or null when the stream ends between messages. */
    fun next(): ControlMessage? {
        val lines = ArrayList<String>()
        var code = -1
        while (true) {
            val raw = line(first = lines.isEmpty()) ?: return null
            if (raw.length < 4) throw TorControlException("a line of ${raw.length} characters is shorter than a code and a separator")
            val c = raw.substring(0, 3)
            if (!c.all { it in '0'..'9' }) throw TorControlException("a line does not start with a three-digit code")
            val n = c.toInt()
            if (code != -1 && n != code) throw TorControlException("the code changes inside one reply")
            code = n
            if (lines.size >= TorControl.MAX_LINES) throw TorControlException("a reply has more than ${TorControl.MAX_LINES} lines")
            lines.add(raw.substring(4))
            when (raw[3]) {
                ' ' -> return ControlMessage(code, lines)
                '-' -> {}
                '+' -> throw TorControlException("a data reply is not accepted")
                else -> throw TorControlException("a line has a separator that is not a space or a dash")
            }
        }
    }

    /** One line without its line end; null at the end of the stream when [first] is true and nothing was read. */
    private fun line(first: Boolean): String? {
        val sb = StringBuilder()
        var sawCr = false
        while (true) {
            val b = input.read()
            if (b < 0) {
                if (first && sb.isEmpty() && !sawCr) return null
                throw TorControlException("the stream ended before a line end")
            }
            if (sawCr) {
                if (b != '\n'.code) throw TorControlException("a carriage return without a line feed")
                return sb.toString()
            }
            when {
                b == '\r'.code -> sawCr = true
                b == '\n'.code -> throw TorControlException("a line feed without a carriage return")
                b < 0x20 || b > 0x7e -> throw TorControlException("a byte that is not printable ASCII")
                else -> {
                    if (sb.length >= TorControl.MAX_LINE) throw TorControlException("a line is longer than ${TorControl.MAX_LINE} bytes")
                    sb.append(b.toChar())
                }
            }
        }
    }
}

/** The five commands, and what the replies mean. */
object TorControl {
    const val CODE_OK = 250
    const val CODE_EVENT = 650
    const val MAX_LINE = 1024
    const val MAX_LINES = 8

    /** Tor is started with no control password and no cookie: the socket is the one the natives give (OA-98). */
    const val AUTHENTICATE = "AUTHENTICATE\r\n"
    const val SET_EVENTS = "SETEVENTS STATUS_CLIENT\r\n"
    const val GET_BOOTSTRAP = "GETINFO status/bootstrap-phase\r\n"
    const val GET_SOCKS = "GETINFO net/listeners/socks\r\n"

    /** Used to stop Tor when the app lets go of it; the only way the natives leave to end a run. */
    const val SHUTDOWN = "SIGNAL SHUTDOWN\r\n"

    /** Refuses a reply that is not a plain success: code 250 and a last line `OK`. */
    fun expectOk(m: ControlMessage) {
        if (m.code != CODE_OK) throw TorControlException("Tor answered ${m.code}, not $CODE_OK")
        if (m.lines.last() != "OK") throw TorControlException("the last line of the reply is not OK")
    }

    /** The value of a GETINFO key in a reply that is a success: `key=value` on one line then `OK`. */
    fun infoValue(m: ControlMessage, key: String): String {
        expectOk(m)
        if (m.lines.size != 2) throw TorControlException("a GETINFO reply is not one value and OK")
        val prefix = "$key="
        if (!m.lines[0].startsWith(prefix)) throw TorControlException("the reply is not for $key")
        return m.lines[0].substring(prefix.length)
    }

    /**
     * The port of the one SOCKS listener, from the reply to [GET_SOCKS]: the value must be exactly one quoted
     * `127.0.0.1:port` with a port from 1 to 65535. Another address, a second listener or none is refused.
     */
    fun socksPort(m: ControlMessage): Int {
        val v = infoValue(m, "net/listeners/socks")
        val host = "\"127.0.0.1:"
        if (!v.startsWith(host) || !v.endsWith("\"")) throw TorControlException("the SOCKS listener is not one quoted 127.0.0.1 address")
        val digits = v.substring(host.length, v.length - 1)
        if (digits.isEmpty() || digits.length > 5 || !digits.all { it in '0'..'9' }) throw TorControlException("the SOCKS port is not a number")
        val port = digits.toInt()
        if (port !in 1..65535) throw TorControlException("the SOCKS port is out of range")
        return port
    }

    /** The progress, 0 to 100, in the reply to [GET_BOOTSTRAP]. */
    fun bootstrapProgress(m: ControlMessage): Int {
        val v = infoValue(m, "status/bootstrap-phase")
        return progressOf(v.split(' ')) ?: throw TorControlException("the bootstrap phase has no progress")
    }

    /** What a `STATUS_CLIENT` event says. */
    sealed class Status {
        /** A step of the bootstrap, 0 to 100. */
        class Bootstrap(val percent: Int) : Status()

        /** The first circuit is built: Tor can be used. */
        object CircuitEstablished : Status()

        /** Any other status of the client: nothing to do. */
        object Other : Status()
    }

    /** Reads an event of code 650. An event that is not a single `STATUS_CLIENT` line is refused. */
    fun status(m: ControlMessage): Status {
        if (!m.isEvent || m.lines.size != 1) throw TorControlException("an event is not one line of code $CODE_EVENT")
        val tokens = m.lines[0].split(' ')
        if (tokens.size < 3 || tokens[0] != "STATUS_CLIENT") throw TorControlException("an event that is not STATUS_CLIENT")
        return when (tokens[2]) {
            "CIRCUIT_ESTABLISHED" -> Status.CircuitEstablished
            "BOOTSTRAP" -> Status.Bootstrap(progressOf(tokens) ?: throw TorControlException("a bootstrap event has no progress"))
            else -> Status.Other
        }
    }

    private fun progressOf(tokens: List<String>): Int? {
        val t = tokens.firstOrNull { it.startsWith("PROGRESS=") } ?: return null
        val digits = t.substring("PROGRESS=".length)
        if (digits.isEmpty() || digits.length > 3 || !digits.all { it in '0'..'9' }) return null
        return digits.toInt().takeIf { it <= 100 }
    }
}
