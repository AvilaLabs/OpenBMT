package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerNetwork

/** The config file did not have the form this app writes. */
class ConfigFormatException(message: String) : IOException(message)

/** How the app reaches Tor (OA-99). */
enum class TorChoice(val key: String) {
    /** The app runs Tor itself (OA-97). The default of a new setup. */
    BuiltIn("builtin"),

    /** Everything goes through Orbot at the address of the config. */
    Orbot("orbot");

    /** The words of this choice, for the page. */
    val words: SayWords get() = when (this) {
        BuiltIn -> AppWords.torBuiltIn
        Orbot -> AppWords.torUsesOrbot
    }

    /** The label of the button that picks it. */
    val label: String get() = when (this) {
        BuiltIn -> "Built-in Tor"
        Orbot -> "Use Orbot"
    }

    companion object {
        fun fromKey(key: String): TorChoice? = values().firstOrNull { it.key == key }
    }
}

/**
 * What the owner app keeps besides the sealed setup, in one small text file: the address of Orbot, as the line
 * `proxy: host:port`, and the choice of Tor, as the line `tor: builtin` or `tor: orbot` (OA-99), written through
 * [DurableFile]. A file of one line, as earlier builds wrote it, is a setup that uses Orbot at that address.
 * Nothing in it is secret.
 */
class AppConfig(val proxy: String, val tor: TorChoice) {
    fun encode(): ByteArray = "proxy: $proxy\ntor: ${tor.key}\n".toByteArray(Charsets.UTF_8)

    /** The same config with another choice; the address of Orbot is kept. */
    fun with(tor: TorChoice): AppConfig = AppConfig(proxy, tor)

    companion object {
        const val DEFAULT_PROXY = "127.0.0.1:9050"

        /** What a new setup is saved with (OA-99). */
        fun fresh(): AppConfig = AppConfig(DEFAULT_PROXY, TorChoice.BuiltIn)

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
            if (lines.size != 1 && lines.size != 2) throw ConfigFormatException("the config file does not have 1 or 2 lines")
            val prefix = "proxy: "
            if (!lines[0].startsWith(prefix)) throw ConfigFormatException("line 1 is not \"proxy: ...\"")
            val proxy = lines[0].substring(prefix.length)
            if (!HostPort.valid(proxy)) throw ConfigFormatException("the proxy is not host:port")
            if (lines.size == 1) return AppConfig(proxy, TorChoice.Orbot)
            val torPrefix = "tor: "
            if (!lines[1].startsWith(torPrefix)) throw ConfigFormatException("line 2 is not \"tor: ...\"")
            val tor = TorChoice.fromKey(lines[1].substring(torPrefix.length)) ?: throw ConfigFormatException("the choice of Tor is not builtin or orbot")
            return AppConfig(proxy, tor)
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

    /** The Keystore key that seals the watch files (OA-81). Its own key, no PIN: the background watch opens them. */
    const val WATCH_KEY_ALIAS = "openbmt-owner-watch-v1"

    /** The sealed secret of the watch key: 32 random bytes, drawn on this phone and derived from nothing (OA-81). */
    const val WATCH_SECRET_FILE = "watch-key.obks"

    /** The sealed contents the background watcher starts from: network, proxy, relay, counters and the secret; no wallet, no owner key (OA-81). */
    const val WATCH_FILE = "watch.obks"

    /** Its presence is the setting "Watch in the background" being on (OA-80). */
    const val WATCH_ON_FILE = "watch-on"

    /**
     * The Keystore key that seals the own-wallet shop's secrets (OA-105): the owner root's secret and the wallet's
     * description. Its own key, so that nothing of the other setups is opened with it.
     */
    const val OWN_KEY_ALIAS = "openbmt-owner-own-wallet-v1"

    /** The sealed wallet file of a shop with its own wallet, as the library made it (`OBM)`, OA-105). */
    const val OWN_WALLET_FILE = "own-wallet.obks"

    /** The sealed secret of the owner root of a shop with its own wallet: 32 random bytes drawn by the library (OA-105). */
    const val OWN_OWNER_FILE = "own-owner.obks"

    /** The sealed link of a shop with its own wallet to its Box: the Box's address and key, and the counters that still wait for the relay code (BX-18). */
    const val OWN_LINK_FILE = "own-link.obks"

    /** The folder of the own-wallet shop's state (the library's `own-shop` file, the allocation book and the range outbox are kept in it). */
    const val OWN_DIR = "own-shop-dir"

    /** The shop's name, money and till, and whether the till steps were shown: the first screens' own file. It holds no secret. */
    const val SHOP_FILE = "shop.txt"

    /** The node's own folder, inside the app's `noBackupFilesDir`. */
    const val NODE_DIR = "node"
}

/** The config file of the app's own folder. */
class ConfigFile(private val dir: File, name: String = "config.txt") {
    private val file = File(dir, name)

    fun load(): AppConfig? = if (file.exists()) AppConfig.decode(file.readBytes()) else null

    fun save(config: AppConfig) = DurableFile.write(dir, file, config.encode())
}
