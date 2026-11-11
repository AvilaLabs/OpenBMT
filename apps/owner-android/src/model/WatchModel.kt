package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import java.security.SecureRandom
import org.openbmt.app.Sealer
import org.openbmt.app.SealedFile
import org.openbmt.app.SayWords

// The pure parts of "Watch in the background" (OA-80 to OA-89): whether the notification permission must be
// asked, the watch key's secret and its two sealed files, the setting's on-marker, what the notification
// says, and the pace of the watcher. No Android class is used here; the service is in `WatchService.kt`.

/** The notification permission (OA-83). */
object WatchPermission {
    /** The first Android version that asks for it. */
    const val ASKED_FROM_SDK = 33

    /** True when the permission must be asked for before the setting can turn on. */
    fun mustAsk(sdk: Int, granted: Boolean): Boolean = sdk >= ASKED_FROM_SDK && !granted

    /** The words when the permission is refused: the setting stays off (OA-83). */
    val refused: SayWords get() = AppWords.watchNotificationsOff
}

/** What the notification says (OA-82, OA-88). */
object WatchNotice {
    enum class State { Watching, NoOrbot }

    fun text(state: State): String = when (state) {
        State.Watching -> AppWords.watchNotificationText
        State.NoOrbot -> AppWords.watchNotificationNoOrbot
    }
}

/** The pace of the background watcher (OA-87): the library's own step, no wake lock, no alarm. */
object WatchSchedule {
    /** The longest one step waits for the network, in milliseconds. */
    const val STEP_WAIT_MS = 1000L

    /** How long the loop sleeps while the app is on screen and the watcher has nothing to do. */
    const val IDLE_MS = 500L

    /** How long to wait before asking Orbot again after it did not answer (OA-88). */
    const val ORBOT_RETRY_MS = 30_000L

    /** Milliseconds until Orbot may be asked again: 0 when it may. */
    fun orbotWaitMs(lastTryMs: Long?, nowMs: Long): Long {
        if (lastTryMs == null) return 0
        return (lastTryMs + ORBOT_RETRY_MS - nowMs).coerceIn(0, ORBOT_RETRY_MS)
    }
}

/** The watch key's secret: 32 random bytes of this phone, never derived from a wallet or an identity key (OA-81). */
object WatchSecret {
    const val LEN = 32

    /**
     * A new secret and its public key. [makeKey] is the library's: it refuses bytes that are not a key, in which
     * case a new draw is made (it is not a thing that happens, but it is not forbidden).
     */
    fun draw(random: SecureRandom, makeKey: (ByteArray) -> ByteArray): Pair<ByteArray, ByteArray>? {
        repeat(3) {
            val secret = ByteArray(LEN)
            random.nextBytes(secret)
            try {
                return secret to makeKey(secret)
            } catch (_: org.openbmt.owner.OwnerException) {
                secret.fill(0)
            }
        }
        return null
    }
}

/**
 * The watch key's secret and the contents the watcher starts from, each in its own file of the app's
 * `noBackupFilesDir`, sealed with the Keystore key [AppFiles.WATCH_KEY_ALIAS] and no PIN (OA-81); and the
 * marker file whose presence is the setting being on (OA-80). Nothing here shows a secret.
 */
class WatchStore(private val dir: File, sealer: Sealer) {
    private val secretFile = SealedFile(dir, sealer, AppFiles.WATCH_SECRET_FILE)
    private val contents = SealedFile(dir, sealer, AppFiles.WATCH_FILE)
    private val marker = File(dir, AppFiles.WATCH_ON_FILE)

    /** True when the setting is on. */
    fun enabled(): Boolean = marker.exists()

    /** True when the watch key's secret is kept. */
    fun hasKey(): Boolean = File(dir, AppFiles.WATCH_SECRET_FILE).exists()

    fun saveSecret(secret: ByteArray) = secretFile.save(secret)

    /** The secret, or null when none is kept. A file that does not read or does not open throws. */
    fun loadSecret(): ByteArray? = secretFile.load()

    /** The contents for the watcher, rewritten at each hand-over so that its counters are the node's now. */
    fun saveContents(bytes: ByteArray) = contents.save(bytes)

    fun loadContents(): ByteArray? = contents.load()

    fun setEnabled(on: Boolean) {
        if (on) {
            if (!marker.exists() && !marker.createNewFile()) throw IOException("the setting could not be saved")
        } else if (marker.exists() && !marker.delete()) {
            throw IOException("the setting could not be turned off")
        }
    }

    /** Turns the setting off and deletes both sealed files. */
    fun remove() {
        setEnabled(false)
        for (name in listOf(AppFiles.WATCH_SECRET_FILE, AppFiles.WATCH_FILE)) {
            val f = File(dir, name)
            if (f.exists() && !f.delete()) throw IOException("a watch file could not be removed")
        }
    }
}
