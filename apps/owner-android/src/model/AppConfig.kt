package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile
import org.openbmt.owner.OwnerNetwork

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

        /**
         * The network a wallet made on this phone is for. It is the build's one choice: the app has no
         * picker, and the Key app's answer must be for the same network or the library refuses it.
         */
        val NETWORK: OwnerNetwork = OwnerNetwork.Mainnet

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

    /**
     * The sealed record of a key ceremony that is not finished (OA-26): key 1's secret and the owner's secret,
     * sealed under the same Keystore key as the setup. It is deleted once the setup is saved (OA-29).
     */
    const val CEREMONY_FILE = "ceremony.obks"

    /** The Keystore key that seals the till's token (OA-16). Its own key: removing the till does not touch the setup's. */
    const val POS_KEY_ALIAS = "openbmt-owner-pos-token-v1"

    /** The sealed read-only token of the till (OA-16). */
    const val POS_TOKEN_FILE = "pos-token.obks"

    /**
     * Present when key 2 of the saved setup is held on a hardware signer, as the ceremony's pairs of keys said
     * (OA-72). It holds no secret. The setup text has no line for it, so a setup that is pasted has none.
     */
    const val KEY2_SIGNER_FILE = "key2-signer.txt"

    /** The record of the first address check (AS-99), as the library framed it. It holds no secret. */
    const val FIRST_ADDRESS_FILE = "first-address.obm0"

    /** The node's own folder, inside the app's `noBackupFilesDir`. */
    const val NODE_DIR = "node"
}

/** The config file of the app's own folder. */
class ConfigFile(private val dir: File, name: String = "config.txt") {
    private val file = File(dir, name)

    fun load(): AppConfig? = if (file.exists()) AppConfig.decode(file.readBytes()) else null

    fun save(config: AppConfig) = DurableFile.write(dir, file, config.encode())
}
