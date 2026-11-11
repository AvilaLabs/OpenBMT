package org.openbmt.counter.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile

/** The config file did not have the form this app writes. */
class ConfigFormatException(message: String) : IOException(message)

/**
 * What the counter keeps besides the pairing and the PIN, in one small text
 * file: the address of Orbot, the manager's name and the cashier's name. The
 * lines are `proxy: host:port`, `manager: name` and `cashier: name` (the last
 * may be empty), one each, in that order, written through [DurableFile].
 * Nothing in it is secret.
 */
class AppConfig(val proxy: String, val manager: String, val cashier: String) {
    fun withCashier(name: String) = AppConfig(proxy, manager, name)

    fun encode(): ByteArray = "proxy: $proxy\nmanager: $manager\ncashier: $cashier\n".toByteArray(Charsets.UTF_8)

    companion object {
        const val DEFAULT_PROXY = "127.0.0.1:9050"
        const val MAX_NAME = 64

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
            if (lines.size != 3) throw ConfigFormatException("the config file does not have 3 lines")
            fun value(i: Int, key: String): String {
                val prefix = "$key: "
                val line = lines[i]
                if (!line.startsWith(prefix)) throw ConfigFormatException("line ${i + 1} is not \"$key: ...\"")
                return line.substring(prefix.length)
            }
            val proxy = value(0, "proxy")
            val manager = value(1, "manager")
            val cashier = value(2, "cashier")
            if (!HostPort.valid(proxy)) throw ConfigFormatException("the proxy is not host:port")
            if (!validName(manager)) throw ConfigFormatException("the manager's name is not a name")
            if (cashier.isNotEmpty() && !validName(cashier)) throw ConfigFormatException("the cashier's name is not a name")
            return AppConfig(proxy, manager, cashier)
        }
    }
}

/** The config file of the app's own folder. */
class ConfigFile(private val dir: File, name: String = "config.txt") {
    private val file = File(dir, name)

    fun load(): AppConfig? = if (file.exists()) AppConfig.decode(file.readBytes()) else null

    fun save(config: AppConfig) = DurableFile.write(dir, file, config.encode())
}
