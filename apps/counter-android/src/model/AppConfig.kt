package org.openbmt.counter.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile

/** The config file did not have the form this app writes. */
class ConfigFormatException(message: String) : IOException(message)

/**
 * What the counter keeps besides the pairing and the PIN, in one small text
 * file: the address of Orbot, the manager's name and the cashier's name, and
 * the address of the shop's own node (CR-100). The lines are
 * `proxy: host:port`, `manager: name`, `cashier: name` and `ownnode: address`
 * (the last two may be empty), one each, in that order, written through
 * [DurableFile]. A fifth line, `sounds: off`, is there only while the sounds
 * are switched off; a file without it has them on. A file of three lines, from
 * before the own node, reads as one with no own node. Nothing in it is secret.
 */
class AppConfig(
    val proxy: String,
    val manager: String,
    val cashier: String,
    val ownNode: String = "",
    /** Whether the app plays its sounds (the phone's own ringer setting still wins). */
    val sounds: Boolean = true,
) {
    fun withCashier(name: String) = AppConfig(proxy, manager, name, ownNode, sounds)

    /** The address of Orbot, from the Network screen (CR-87). */
    fun withProxy(address: String) = AppConfig(address, manager, cashier, ownNode, sounds)

    /** The own node's address as the library made it (CR-100), or "" for none. */
    fun withOwnNode(address: String) = AppConfig(proxy, manager, cashier, address, sounds)

    /** The sounds on or off, from the Sounds screen. */
    fun withSounds(on: Boolean) = AppConfig(proxy, manager, cashier, ownNode, on)

    fun encode(): ByteArray =
        ("proxy: $proxy\nmanager: $manager\ncashier: $cashier\nownnode: $ownNode\n" + if (sounds) "" else "sounds: off\n")
            .toByteArray(Charsets.UTF_8)

    companion object {
        const val DEFAULT_PROXY = "127.0.0.1:9050"
        const val MAX_NAME = 64
        const val MAX_OWN_NODE = 128

        /** An own node's text is empty or one word of at most [MAX_OWN_NODE] characters; the library judges the rest. */
        fun validOwnNode(text: String): Boolean =
            text.length <= MAX_OWN_NODE && text.none { Character.isISOControl(it) || it.isWhitespace() }

        /** A name is 1 to [MAX_NAME] characters, at most 255 bytes, with no control character (the labels of AS-07). */
        fun validName(name: String): Boolean =
            name.isNotEmpty() && name.length <= MAX_NAME && name.toByteArray(Charsets.UTF_8).size <= 255 &&
                name.none { Character.isISOControl(it) } && name == name.trim()

        fun decode(bytes: ByteArray): AppConfig {
            if (bytes.size > 1024) throw ConfigFormatException("the config file is longer than 1024 bytes")
            val text = try {
                Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString()
            } catch (_: java.nio.charset.CharacterCodingException) {
                throw ConfigFormatException("the config file is not UTF-8")
            }
            if (!text.endsWith("\n")) throw ConfigFormatException("the config file does not end with a line end")
            val lines = text.dropLast(1).split('\n')
            if (lines.size < 3 || lines.size > 5) throw ConfigFormatException("the config file does not have 3 to 5 lines")
            fun value(i: Int, key: String): String {
                val prefix = "$key: "
                val line = lines[i]
                if (!line.startsWith(prefix)) throw ConfigFormatException("line ${i + 1} is not \"$key: ...\"")
                return line.substring(prefix.length)
            }
            val proxy = value(0, "proxy")
            val manager = value(1, "manager")
            val cashier = value(2, "cashier")
            val ownNode = if (lines.size >= 4) value(3, "ownnode") else ""
            val sounds = if (lines.size == 5) {
                when (value(4, "sounds")) {
                    "off" -> false
                    else -> throw ConfigFormatException("the sounds line is not \"sounds: off\"")
                }
            } else {
                true
            }
            if (!HostPort.valid(proxy)) throw ConfigFormatException("the proxy is not host:port")
            if (!validName(manager)) throw ConfigFormatException("the manager's name is not a name")
            if (cashier.isNotEmpty() && !validName(cashier)) throw ConfigFormatException("the cashier's name is not a name")
            if (!validOwnNode(ownNode)) throw ConfigFormatException("the own node's address is not one word")
            return AppConfig(proxy, manager, cashier, ownNode, sounds)
        }
    }
}

/** The config file of the app's own folder. */
class ConfigFile(private val dir: File, name: String = "config.txt") {
    private val file = File(dir, name)

    fun load(): AppConfig? = if (file.exists()) AppConfig.decode(file.readBytes()) else null

    fun save(config: AppConfig) = DurableFile.write(dir, file, config.encode())
}
