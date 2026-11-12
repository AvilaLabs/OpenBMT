package org.openbmt.app

import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import org.torproject.jni.TorService

/** The built-in Tor did not come up: it stopped, was let go of, or did not finish connecting in time. */
class TorStartException(message: String) : IOException(message)

/**
 * The built-in Tor of the owner app and the counter app (OA-97 to OA-100, CR-201 to CR-204): one run of [TorService] for the whole process, shared by whoever
 * holds a [TorLease] on it. The first lease starts it, bound to a SOCKS port that Tor picks on 127.0.0.1, with its
 * folders inside the app's `noBackupFilesDir` (OA-98). The last lease let go of shuts it down. The screens hold a
 * lease while the app is on show, and the owner app's background watch holds one while its service runs, so the port stays the
 * same from the node's start to the watcher's (the watcher starts from the proxy the node had, OA-81).
 */
object TorRunner {
    /** The folder of Tor, inside the app's `noBackupFilesDir` (OA-98, CR-202). */
    const val TOR_DIR = "tor"

    private val lock = Any()
    private var current: TorRun? = null

    /** A lease on a run of Tor, started if none is running. [dir] is the app's `noBackupFilesDir`. */
    fun acquire(dir: File): TorLease = synchronized(lock) {
        val run = current?.takeIf { !it.done } ?: TorRun(dir).also { current = it; it.begin() }
        run.holders++
        TorLease(run)
    }

    internal fun release(run: TorRun) = synchronized(lock) {
        run.holders--
        if (run.holders == 0) {
            run.stop()
            if (current === run) current = null
        }
    }
}

/** The right to use the built-in Tor. Let go of it with [close]; closing again does nothing. */
class TorLease internal constructor(private val run: TorRun) : AutoCloseable {
    private var closed = false

    /** True when the run it belongs to has ended, so that this lease will never have a port. */
    val dead: Boolean get() = run.done

    /** The SOCKS port on 127.0.0.1 if Tor has a circuit now, else null. */
    fun port(): Int? = run.portNow()

    /** Waits up to [timeoutMs] for Tor to have a circuit and returns its SOCKS port. Throws [TorStartException] if it cannot. */
    fun awaitPort(timeoutMs: Long): Int = run.await(timeoutMs) { closed }

    override fun close() {
        synchronized(this) {
            if (closed) return
            closed = true
        }
        run.wake()
        TorRunner.release(run)
    }
}

/** One run of Tor and what it has reported. */
class TorRun internal constructor(private val dir: File) : TorService.Listener {
    private val lock = ReentrantLock()
    private val changed = lock.newCondition()
    private var port = 0
    private var ended = false
    private var error: String? = null
    private var service: TorService? = null
    internal var holders = 0

    val done: Boolean get() {
        lock.lock()
        try {
            return ended
        } finally {
            lock.unlock()
        }
    }

    internal fun begin() {
        val torDir = File(dir, TorRunner.TOR_DIR)
        val data = File(torDir, "data")
        val cache = File(torDir, "cache")
        try {
            for (d in listOf(torDir, data, cache)) {
                d.mkdirs()
                // Tor refuses a data directory that others can reach.
                if (!(d.setReadable(true, true) && d.setWritable(true, true) && d.setExecutable(true, true))) {
                    throw IOException("a folder of Tor is not private")
                }
            }
            val s = TorService(data, cache, this)
            service = s
            s.start()
        } catch (e: IOException) {
            onStopped(e.message ?: "Tor could not start")
        } catch (e: LinkageError) {
            // The native library is not in the APK or did not load.
            onStopped(e.message ?: "the library of Tor did not load")
        }
    }

    internal fun stop() {
        service?.stop()
    }

    internal fun wake() {
        lock.lock()
        try {
            changed.signalAll()
        } finally {
            lock.unlock()
        }
    }

    internal fun portNow(): Int? {
        lock.lock()
        try {
            return if (port > 0 && !ended) port else null
        } finally {
            lock.unlock()
        }
    }

    internal fun await(timeoutMs: Long, closed: () -> Boolean): Int {
        val deadline = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(timeoutMs)
        lock.lock()
        try {
            while (true) {
                if (closed()) throw TorStartException("the lease was let go of")
                if (ended) throw TorStartException(error ?: "Tor stopped")
                if (port > 0) return port
                val left = deadline - System.nanoTime()
                if (left <= 0) throw TorStartException("Tor did not connect in time")
                try {
                    changed.awaitNanos(left)
                } catch (_: InterruptedException) {
                    throw TorStartException("interrupted")
                }
            }
        } finally {
            lock.unlock()
        }
    }

    override fun onProgress(percent: Int) {}

    override fun onReady(socksPort: Int) {
        lock.lock()
        try {
            port = socksPort
            changed.signalAll()
        } finally {
            lock.unlock()
        }
    }

    override fun onStopped(error: String?) {
        lock.lock()
        try {
            ended = true
            this.error = error
            port = 0
            changed.signalAll()
        } finally {
            lock.unlock()
        }
    }
}
