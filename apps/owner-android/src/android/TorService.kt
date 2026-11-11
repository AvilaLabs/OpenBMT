package org.torproject.jni

import java.io.File
import java.io.FileDescriptor
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.locks.ReentrantLock
import org.openbmt.owner.app.ControlMessage
import org.openbmt.owner.app.ControlReader
import org.openbmt.owner.app.TorControl
import org.openbmt.owner.app.TorControlException

/**
 * Runs the Tor Project's C Tor inside the app, with the native library of the Guardian Project's `tor-android`
 * 0.4.9.14 (`libtor.so`, pinned in `docs/DEPENDENCIES.md`) and nothing else of that library. The class has the
 * package and the name of the library's own `TorService` because the native functions are bound by those names
 * and read the two fields below by name; it is not an Android service and has none of the library's other code.
 *
 * The order and the threads of the native calls follow `TorService.java` of github.com/guardianproject/tor-android
 * (BSD 3-Clause, Copyright (c) the Guardian Project): the run lock is held from the first call until the
 * configuration is freed; one thread makes the configuration and runs the command line once with
 * `--verify-config`, starts the thread that connects to the control socket, then sets the command line again,
 * sets up the control socket and calls `runMain`, which returns when Tor ends. The differences are these: the
 * lock is taken and released on the thread that runs Tor, the control socket is waited for by looking for its
 * file, the control client is [ControlReader] and the five commands of [TorControl] in place of `jtorctl`, status
 * goes to a plain [Listener] and not to a broadcast, and the command line carries one option for the one SOCKS
 * listener, at 127.0.0.1 on a port Tor picks (OA-97). Nothing is read from a `torrc`.
 */
class TorService(private val dataDir: File, private val cacheDir: File, private val listener: Listener) {
    /** What the run reports. Each call comes from one of the two threads of the run, never from the caller's. */
    interface Listener {
        /** A step of the bootstrap, 0 to 100. */
        fun onProgress(percent: Int)

        /** Tor has a circuit and listens for SOCKS at 127.0.0.1:[socksPort]. */
        fun onReady(socksPort: Int)

        /** Tor is not running any more. [error] is null when it was asked to stop. Called once. */
        fun onStopped(error: String?)
    }

    // Read and written by the native code, by these names and these types.
    @Suppress("unused")
    private var torConfiguration: Long = -1

    @Suppress("unused")
    private var torControlFd: Int = -1

    private external fun createTorConfiguration(): Boolean

    private external fun mainConfigurationFree()

    private external fun mainConfigurationSetCommandLine(args: Array<String>): Boolean

    private external fun mainConfigurationSetupControlSocket(): Boolean

    private external fun runMain(): Int

    private val socket = File(dataDir, CONTROL_SOCKET_NAME)
    private val writeLock = Any()
    private var out: FileOutputStream? = null
    private var shutdownSent = false

    @Volatile
    private var stopRequested = false
    private val stopped = AtomicBoolean(false)
    private var socksPort = 0
    private var established = false
    private var readyFired = false

    @Volatile
    private var controlStarted: CountDownLatch? = null

    /** Starts Tor on its own thread. Called once. */
    fun start() {
        torThread.start()
    }

    /** Asks Tor to shut down; the listener hears [Listener.onStopped] when it has. Does not wait. */
    fun stop() {
        stopRequested = true
        shutdownIfRequested()
    }

    private fun shutdownIfRequested() {
        synchronized(writeLock) {
            val o = out ?: return
            if (!stopRequested || shutdownSent) return
            shutdownSent = true
            try {
                o.write(TorControl.SHUTDOWN.toByteArray(Charsets.US_ASCII))
                o.flush()
            } catch (_: IOException) {
                // The socket is gone: Tor is ending anyway.
            }
        }
    }

    private fun finish(error: String?) {
        if (stopped.compareAndSet(false, true)) listener.onStopped(error)
    }

    private val torThread = Thread({ runTor() }, "tor")

    private fun runTor() {
        runLock.lock()
        var error: String? = null
        var made = false
        try {
            if (stopRequested) return
            socket.delete()
            made = createTorConfiguration()
            if (!made) throw IllegalStateException("the configuration was not made")
            val torrc = File(dataDir.parentFile, "torrc")
            val defaults = File(dataDir.parentFile, "torrc-defaults")
            val lines = listOf(
                "tor", "--verify-config", // must always be here, as the library's TorService has it
                "--RunAsDaemon", "0",
                "-f", torrc.absolutePath,
                "--defaults-torrc", defaults.absolutePath,
                "--ignore-missing-torrc",
                "--CacheDirectory", cacheDir.absolutePath,
                "--DataDirectory", dataDir.absolutePath,
                "--ControlSocket", socket.absolutePath,
                "--CookieAuthentication", "0",
                "--SocksPort", "127.0.0.1:auto",
            )
            val verify = lines.toTypedArray()
            if (!mainConfigurationSetCommandLine(verify)) throw IllegalArgumentException("the command line was not set")
            if (runMain() != 0) throw IllegalArgumentException("the command line was not accepted")

            val started = CountDownLatch(1)
            controlStarted = started
            controlThread.start()
            started.await()

            if (stopRequested) return
            // The same line without the first option, which is the check.
            val run = (listOf("tor") + lines.drop(2)).toTypedArray()
            if (!mainConfigurationSetCommandLine(run)) throw IllegalArgumentException("the command line was not set")
            if (!mainConfigurationSetupControlSocket()) throw IllegalStateException("the control socket was not set up")
            if (runMain() != 0 && !stopRequested) throw IllegalStateException("Tor ended with an error")
        } catch (e: IllegalStateException) {
            error = e.message ?: "Tor did not start"
        } catch (e: IllegalArgumentException) {
            error = e.message ?: "Tor did not start"
        } catch (_: InterruptedException) {
            error = "interrupted"
        } finally {
            if (made) mainConfigurationFree()
            runLock.unlock()
            finish(error ?: if (stopRequested) null else "Tor ended")
        }
    }

    /** Waits for Tor to make its control socket, then talks to it (OA-97). */
    private val controlThread = Thread({ runControl() }, CONTROL_SOCKET_NAME)

    private fun runControl() {
        android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_BACKGROUND)
        controlStarted?.countDown()
        try {
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(SOCKET_WAIT_S)
            while (!socket.exists()) {
                if (System.nanoTime() > deadline) throw IOException("Tor made no control socket")
                Thread.sleep(50)
            }
            val fd = prepareFileDescriptor(socket.absolutePath)
            val input = FileInputStream(fd)
            val output = FileOutputStream(fd)
            synchronized(writeLock) { out = output }
            val reader = ControlReader(input)
            TorControl.expectOk(command(reader, output, TorControl.AUTHENTICATE))
            TorControl.expectOk(command(reader, output, TorControl.SET_EVENTS))
            shutdownIfRequested()
            socksPort = TorControl.socksPort(command(reader, output, TorControl.GET_SOCKS))
            val progress = TorControl.bootstrapProgress(command(reader, output, TorControl.GET_BOOTSTRAP))
            listener.onProgress(progress)
            if (progress >= 100) established = true
            maybeReady()
            while (true) {
                val m = reader.next() ?: break
                if (m.isEvent) {
                    handle(m)
                } else if (!stopRequested) {
                    throw TorControlException("a reply that was not asked for")
                }
            }
            if (!stopRequested) throw IOException("the control socket closed")
        } catch (e: IOException) {
            fail(e.message ?: "the control socket failed")
        } catch (_: InterruptedException) {
            fail("interrupted")
        }
    }

    /** Sends one command and returns its reply; events that come first are handled. */
    private fun command(reader: ControlReader, output: FileOutputStream, text: String): ControlMessage {
        synchronized(writeLock) {
            output.write(text.toByteArray(Charsets.US_ASCII))
            output.flush()
        }
        while (true) {
            val m = reader.next() ?: throw IOException("the control socket closed")
            if (m.isEvent) handle(m) else return m
        }
    }

    private fun handle(m: ControlMessage) {
        when (val s = TorControl.status(m)) {
            is TorControl.Status.Bootstrap -> {
                listener.onProgress(s.percent)
                if (s.percent >= 100) established = true
            }
            TorControl.Status.CircuitEstablished -> established = true
            TorControl.Status.Other -> {}
        }
        maybeReady()
    }

    private fun maybeReady() {
        if (readyFired || !established || socksPort == 0) return
        readyFired = true
        listener.onReady(socksPort)
    }

    /** The control socket could not be used: Tor is asked to stop, and the run reports the error. */
    private fun fail(error: String) {
        stopRequested = true
        shutdownIfRequested()
        finish(error)
    }

    companion object {
        private const val CONTROL_SOCKET_NAME = "ControlSocket"
        private const val SOCKET_WAIT_S = 15L

        init {
            System.loadLibrary("tor")
        }

        /** The library's native code runs one Tor at a time in a process: this is held from the first call to the last. */
        private val runLock = ReentrantLock()

        /** Opens the control socket at [path] and returns the file descriptor of the connection. */
        @JvmStatic
        private external fun prepareFileDescriptor(path: String): FileDescriptor
    }
}
