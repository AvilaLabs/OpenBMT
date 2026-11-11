package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile

/** The config file did not have the form this app writes. */
class ConfigFormatException(message: String) : IOException(message)

/**
 * What the owner app keeps besides the sealed setup, in one small text file:
 * the address of Orbot, as the one line `proxy: host:port`, written through
 * [DurableFile]. Nothing in it is secret.
 */
class AppConfig(val proxy: String) {
    fun encode(): ByteArray = "proxy: $proxy\n".toByteArray(Charsets.UTF_8)

    companion object {
        const val DEFAULT_PROXY = "127.0.0.1:9050"

        fun decode(bytes: ByteArray): AppConfig {
            if (bytes.size > 256) throw ConfigFormatException("the config file is longer than 256 bytes")
            val text = try {
                Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString()
            } catch (_: java.nio.charset.CharacterCodingException) {
                throw ConfigFormatException("the config file is not UTF-8")
            }
            if (!text.endsWith("\n")) throw ConfigFormatException("the config file does not end with a line end")
            val lines = text.dropLast(1).split('\n')
            if (lines.size != 1) throw ConfigFormatException("the config file does not have 1 line")
            val prefix = "proxy: "
            if (!lines[0].startsWith(prefix)) throw ConfigFormatException("line 1 is not \"proxy: ...\"")
            val proxy = lines[0].substring(prefix.length)
            if (!HostPort.valid(proxy)) throw ConfigFormatException("the proxy is not host:port")
            return AppConfig(proxy)
        }
    }
}

/** The names of what the app keeps in its own folder, and of the key that seals the setup. */
object AppFiles {
    /** The Keystore key that seals the setup (OA-01). */
    const val KEY_ALIAS = "openbmt-owner-setup-v1"

    /** The sealed setup text: the owner's secret and Key 1's account key (OA-01). */
    const val SETUP_FILE = "setup.obks"

    /** The node's own folder, inside the app's `noBackupFilesDir`. */
    const val NODE_DIR = "node"
}

/** The config file of the app's own folder. */
class ConfigFile(private val dir: File, name: String = "config.txt") {
    private val file = File(dir, name)

    fun load(): AppConfig? = if (file.exists()) AppConfig.decode(file.readBytes()) else null

    fun save(config: AppConfig) = DurableFile.write(dir, file, config.encode())
}
