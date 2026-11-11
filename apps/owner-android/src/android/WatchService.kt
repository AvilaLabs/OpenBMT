package org.openbmt.owner.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.os.Build
import android.os.IBinder
import java.io.File
import org.openbmt.app.KeystoreSealer
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerWatch

/**
 * "Watch in the background" (OA-80 to OA-89): a foreground service of type `specialUse` with a visible, ongoing
 * notification. It is started while the app is on screen, when the setting is on, and does nothing until the
 * controller hands the wallet over as the app is hidden ([WatchRunner.handOver]); it gives it back when the app
 * is shown again. It is never sticky, never started by a boot, and holds no wake lock and no alarm (OA-87).
 * What it runs is [OwnerWatch]: the watch key and the counters, and no wallet secret and no owner key (OA-81).
 */
class WatchService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            turnOff()
            return START_NOT_STICKY
        }
        show(WatchNotice.State.Watching, first = true)
        WatchRunner.begin(noBackupFilesDir) { state -> show(state, first = false) }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        WatchRunner.end()
        super.onDestroy()
    }

    /** The notification's Stop (OA-85): the setting is off, the watcher stops, the service ends. */
    private fun turnOff() {
        try {
            WatchStore(noBackupFilesDir, KeystoreSealer(AppFiles.WATCH_KEY_ALIAS)).setEnabled(false)
        } catch (_: java.io.IOException) {
            // The service ends anyway; the app shows the setting as it finds it.
        }
        WatchRunner.end()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun show(state: WatchNotice.State, first: Boolean) {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, AppWords.watchNotificationTitle, NotificationManager.IMPORTANCE_LOW),
        )
        val stop = PendingIntent.getService(
            this,
            0,
            Intent(this, WatchService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        // The app has no icon of its own to draw in the status bar; the system's is used.
        val icon = android.R.drawable.ic_dialog_info
        val n = Notification.Builder(this, CHANNEL)
            .setSmallIcon(icon)
            .setContentTitle(AppWords.watchNotificationTitle)
            .setContentText(WatchNotice.text(state))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .addAction(Notification.Action.Builder(Icon.createWithResource(this, icon), AppWords.watchNotificationStop, stop).build())
            .build()
        if (first) {
            if (Build.VERSION.SDK_INT >= 34) {
                startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
            } else {
                startForeground(NOTIFICATION_ID, n)
            }
        } else {
            manager.notify(NOTIFICATION_ID, n)
        }
    }

    companion object {
        const val ACTION_STOP = "org.openbmt.owner.app.WATCH_STOP"
        private const val CHANNEL = "watch"
        private const val NOTIFICATION_ID = 80

        /** Starts the service; called only while the app is on screen, which Android allows for a foreground service. */
        fun start(context: Context) {
            context.startForegroundService(Intent(context, WatchService::class.java))
        }

        /** Ends the service and waits for the watcher to let go of the node's folder. */
        fun stop(context: Context) {
            WatchRunner.end()
            context.stopService(Intent(context, WatchService::class.java))
        }
    }
}

/**
 * The thread of the background watch. It runs [OwnerWatch.step] in a loop while the wallet is handed over, and
 * nothing while the app is on screen (the node runs then). Orbot is asked before the watcher starts, and again
 * about every 30 seconds while it does not answer (OA-88). One lock keeps a hand-over, a reclaim and a step from
 * running together; a step waits for the network for at most one second, so a reclaim waits for that long.
 */
object WatchRunner {
    private val lock = Any()
    private var handed = false
    private var watch: OwnerWatch? = null
    private var thread: Thread? = null

    @Volatile
    private var ended = false
    private var lastOrbotTry: Long? = null

    fun begin(dir: File, onState: (WatchNotice.State) -> Unit) {
        synchronized(lock) {
            if (thread?.isAlive == true) return
            ended = false
            val t = Thread { loop(dir, onState) }
            t.isDaemon = true
            thread = t
            t.start()
        }
    }

    /** The app is hidden and has stopped its node: the watcher takes the wallet. */
    fun handOver() = synchronized(lock) { handed = true }

    /** The app is on screen: the watcher lets go of the node's folder, and waits (a step is at most one second). */
    fun reclaim() = synchronized(lock) {
        handed = false
        stopWatch()
    }

    /** Ends the thread and the watcher. */
    fun end() {
        ended = true
        val t = synchronized(lock) {
            handed = false
            stopWatch()
            thread.also { thread = null }
        }
        t?.interrupt()
        try {
            t?.join(1500)
        } catch (_: InterruptedException) {
        }
    }

    private fun stopWatch() {
        watch?.let {
            try {
                it.stop()
            } catch (_: OwnerException) {
            } catch (_: LinkageError) {
            }
        }
        watch = null
    }

    private fun loop(dir: File, onState: (WatchNotice.State) -> Unit) {
        var shown = WatchNotice.State.Watching
        while (!ended) {
            var idle = true
            synchronized(lock) {
                if (handed && !ended) {
                    idle = false
                    shown = step(dir, shown, onState)
                }
            }
            try {
                Thread.sleep(if (idle) WatchSchedule.IDLE_MS else 0L)
            } catch (_: InterruptedException) {
                return
            }
        }
    }

    /** One turn, with the lock held. Returns the state the notification now shows. */
    private fun step(dir: File, shown: WatchNotice.State, onState: (WatchNotice.State) -> Unit): WatchNotice.State {
        var state = shown
        if (watch == null) {
            val now = System.currentTimeMillis()
            if (WatchSchedule.orbotWaitMs(lastOrbotTry, now) > 0) {
                try {
                    Thread.sleep(WatchSchedule.IDLE_MS)
                } catch (_: InterruptedException) {
                }
                return state
            }
            lastOrbotTry = now
            val proxy = try {
                ConfigFile(dir).load()?.proxy
            } catch (_: java.io.IOException) {
                null
            }
            // No Orbot, no watcher: nothing is sent around the proxy (OA-05, OA-88).
            if (proxy == null || !TorGate(proxy).available()) return notify(WatchNotice.State.NoOrbot, state, onState)
            try {
                val bytes = WatchStore(dir, KeystoreSealer(AppFiles.WATCH_KEY_ALIAS)).loadContents() ?: return state
                watch = OwnerWatch.start(bytes)
                lastOrbotTry = null
            } catch (_: OwnerException) {
                return state
            } catch (_: java.io.IOException) {
                return state
            } catch (_: java.security.GeneralSecurityException) {
                return state
            } catch (_: LinkageError) {
                return state
            }
            state = notify(WatchNotice.State.Watching, state, onState)
        }
        val w = watch ?: return state
        try {
            w.step(System.currentTimeMillis() / 1000, WatchSchedule.STEP_WAIT_MS)
        } catch (_: OwnerException) {
            // The watcher stopped itself or could not go on: it starts again after the wait.
            stopWatch()
            lastOrbotTry = System.currentTimeMillis()
        }
        return state
    }

    private fun notify(next: WatchNotice.State, shown: WatchNotice.State, onState: (WatchNotice.State) -> Unit): WatchNotice.State {
        if (next != shown) onState(next)
        return next
    }
}
